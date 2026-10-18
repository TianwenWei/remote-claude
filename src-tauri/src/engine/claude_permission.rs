//! One-call Claude tool approvals for the native SSH client. Original input
//! is retained only in the process registry; IPC carries a display string
//! and a fixed decision, never replacement tool arguments or permission rules.
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

pub(crate) const CONTEXT_KEY: &str = "__ccgui_claude_tool_permission";
pub(crate) const ANSWER_KEY: &str = "__ccgui_tool_permission";
pub(crate) const QUESTION_KEY: &str = "__ccgui_claude_question";

pub(crate) fn parked_context(input: Value) -> Value {
    json!({CONTEXT_KEY:{"input":input}})
}

pub(crate) fn claim_answer(
    questions: &Mutex<HashMap<String, Value>>,
    request_id: &str,
) -> Result<(), String> {
    let mut questions = questions
        .lock()
        .map_err(|_| "Question state is unavailable")?;
    let context = questions
        .get_mut(request_id)
        .and_then(|input| input.get_mut(CONTEXT_KEY))
        .and_then(Value::as_object_mut)
        .ok_or("Tool permission is no longer pending")?;
    if context.get("answerInFlight").and_then(Value::as_bool) == Some(true) {
        return Err("This tool permission is already being answered".into());
    }
    context.insert("answerInFlight".into(), Value::Bool(true));
    Ok(())
}

pub(crate) fn release_answer(questions: &Mutex<HashMap<String, Value>>, request_id: &str) {
    if let Ok(mut questions) = questions.lock() {
        if let Some(context) = questions
            .get_mut(request_id)
            .and_then(|input| input.get_mut(CONTEXT_KEY))
            .and_then(Value::as_object_mut)
        {
            context.remove("answerInFlight");
        }
    }
}

pub(crate) fn answer_frame(
    request_id: &str,
    context: &Value,
    answers: Option<&Value>,
) -> Result<Value, String> {
    let decision = match answers {
        None => "deny",
        Some(Value::Object(answers)) if answers.len() == 1 => answers
            .get(ANSWER_KEY)
            .and_then(Value::as_str)
            .ok_or("Choose Allow once or Deny")?,
        _ => return Err("Choose Allow once or Deny".into()),
    };
    let response = match decision {
        "allow_once" => {
            let input = context
                .get("input")
                .filter(|input| input.is_object())
                .ok_or("Original tool input is unavailable")?;
            json!({"behavior":"allow","updatedInput":input})
        }
        "deny" => json!({"behavior":"deny","message":"The user denied this tool call."}),
        _ => return Err("Choose Allow once or Deny".into()),
    };
    Ok(
        json!({"type":"control_response","response":{"subtype":"success","request_id":request_id,"response":response}}),
    )
}

pub(crate) fn input_display(input: &Value) -> String {
    let text = serde_json::to_string_pretty(input).unwrap_or_else(|_| "[unavailable]".into());
    const LIMIT: usize = 65_536;
    if text.chars().count() <= LIMIT {
        return text;
    }
    let mut display: String = text.chars().take(LIMIT).collect();
    display.push_str("\n… [Input display truncated; approval uses the full original input.]");
    display
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allow_once_preserves_exact_input_without_persistent_permissions() {
        let input =
            json!({"command":"printf '%s' \"hello\"","timeout":120000,"nested":{"emoji":"中文😀"}});
        let context = parked_context(input.clone());
        let answer = answer_frame(
            "request-1",
            &context[CONTEXT_KEY],
            Some(&json!({ANSWER_KEY:"allow_once"})),
        )
        .unwrap();
        assert_eq!(
            answer["response"]["response"],
            json!({"behavior":"allow","updatedInput":input})
        );
        assert_eq!(answer["response"]["request_id"], "request-1");
        assert!(answer["response"]["response"]
            .get("permissionUpdates")
            .is_none());
    }
    #[test]
    fn deny_and_skip_never_return_tool_input() {
        let context = json!({"input":{"command":"touch /tmp/example"}});
        for answers in [None, Some(json!({ANSWER_KEY:"deny"}))] {
            let answer = answer_frame("request-2", &context, answers.as_ref()).unwrap();
            assert_eq!(answer["response"]["response"]["behavior"], "deny");
            assert!(answer["response"]["response"].get("updatedInput").is_none());
        }
    }
    #[test]
    fn invalid_or_ambiguous_answers_do_not_authorize() {
        let context = json!({"input":{"command":"echo example"}});
        for answers in [
            json!({ANSWER_KEY:"Allow once"}),
            json!({ANSWER_KEY:"allow_once","command":"other"}),
            json!({ANSWER_KEY:["allow_once"]}),
            json!("allow_once"),
            json!({"question":"allow_once"}),
        ] {
            assert!(answer_frame("request-3", &context, Some(&answers)).is_err());
        }
    }
    #[test]
    fn truncated_display_preserves_unicode_and_marks_omission() {
        let display = input_display(&json!({"content":"😀".repeat(70_000)}));
        assert!(display.contains("Input display truncated"));
        assert!(display.len() < 300_000);
    }
    #[test]
    fn concurrent_answers_have_one_winner_and_failed_delivery_can_retry() {
        let questions = std::sync::Arc::new(Mutex::new(HashMap::from([(
            "request".into(),
            parked_context(json!({"command":"echo ok"})),
        )])));
        let mut threads = Vec::new();
        for _ in 0..8 {
            let questions = std::sync::Arc::clone(&questions);
            threads.push(std::thread::spawn(move || {
                claim_answer(&questions, "request").is_ok()
            }));
        }
        assert_eq!(
            threads
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .filter(|won| *won)
                .count(),
            1
        );
        release_answer(&questions, "request");
        assert!(claim_answer(&questions, "request").is_ok());
    }
}
