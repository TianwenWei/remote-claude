"""Offline regression checks: no real credentials or network requests."""
import contextlib
import importlib.util
import io
import json
import math
import pathlib
import tempfile
import unittest
import urllib.error
from unittest import mock

SPEC = importlib.util.spec_from_file_location(
    "claude_account_usage", pathlib.Path(__file__).with_name("claude_account_usage.py")
)
usage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(usage)


class Response(io.BytesIO):
    pass


class UsageTests(unittest.TestCase):
    def test_percent_units_nulls_and_reset_times(self):
        normalized = usage.normalize_window({
            "utilization": 41,
            "resets_at": "2026-10-15T14:59:59.627405+00:00",
        })
        self.assertEqual(normalized, {"usedPercent": 41, "resetsAt": 1792076399627})
        self.assertEqual(usage.normalize_window({"utilization": 0}), {"usedPercent": 0, "resetsAt": None})
        for value in [None, {}, {"utilization": None}, {"utilization": True},
                      {"utilization": -1}, {"utilization": 101},
                      {"utilization": math.nan}, {"utilization": math.inf}]:
            self.assertIsNone(usage.normalize_window(value))
        self.assertIsNone(usage.reset_milliseconds("2026-10-15T14:59:59"))
        self.assertIsNone(usage.reset_milliseconds("invalid"))

    def test_only_summary_leaves_vps_and_request_has_fixed_https_origin(self):
        body = {"five_hour": {"utilization": 100}, "seven_day": None,
                "private_server_detail": "must-not-export"}
        opener = mock.Mock()
        opener.open.return_value = Response(json.dumps(body).encode())
        with mock.patch.object(usage, "read_login", return_value=("fake-private-token", "pro")), \
             mock.patch.object(usage.urllib.request, "build_opener", return_value=opener):
            output = usage.fetch_usage()
        request = opener.open.call_args.args[0]
        self.assertEqual(request.full_url, "https://api.anthropic.com/api/oauth/usage")
        self.assertEqual(request.get_method(), "GET")
        self.assertEqual(request.get_header("Authorization"), "Bearer fake-private-token")
        self.assertEqual(opener.open.call_args.kwargs["timeout"], 20)
        self.assertEqual(set(output), {"plan", "fiveHour", "sevenDay", "updatedAt"})
        self.assertNotIn("private", json.dumps(output))
        self.assertIsNone(output["sevenDay"])

    def test_redirects_are_rejected_even_with_same_origin(self):
        handler = usage.NoRedirect()
        for target in ["https://evil.invalid/", "https://api.anthropic.com/other", "http://api.anthropic.com/"]:
            with self.assertRaises(usage.UsageError) as raised:
                handler.redirect_request(None, None, 302, "", {}, target)
            self.assertEqual(raised.exception.code, "redirect")

    def test_oversized_or_invalid_http_body_is_not_exported(self):
        opener = mock.Mock()
        for body in [b"x" * (usage.MAX_RESPONSE_BYTES + 1), b"Bearer private not-json", b"[]"]:
            opener.open.return_value = Response(body)
            with mock.patch.object(usage, "read_login", return_value=("fake", "pro")), \
                 mock.patch.object(usage.urllib.request, "build_opener", return_value=opener):
                with self.assertRaises(usage.UsageError) as raised:
                    usage.fetch_usage()
            self.assertEqual(raised.exception.code, "response_invalid")

    def test_http_failures_return_sanitized_codes_without_reading_body(self):
        for status, expected in [(401, "auth_expired"), (403, "forbidden"), (429, "rate_limited"), (500, "http_error")]:
            private_body = mock.Mock()
            error = urllib.error.HTTPError(usage.USAGE_URL, status, "private-token", {}, private_body)
            opener = mock.Mock()
            opener.open.side_effect = error
            with mock.patch.object(usage, "read_login", return_value=("fake", "pro")), \
                 mock.patch.object(usage.urllib.request, "build_opener", return_value=opener):
                with self.assertRaises(usage.UsageError) as raised:
                    usage.fetch_usage()
            self.assertEqual(raised.exception.code, expected)
            private_body.read.assert_not_called()

    def test_credentials_stay_in_host_config_directory_and_are_never_rewritten(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / ".credentials.json"
            original = json.dumps({"claudeAiOauth": {"accessToken": "fake-private-token", "subscriptionType": "pro"}})
            path.write_text(original)
            with mock.patch.dict(usage.os.environ, {"CLAUDE_CONFIG_DIR": directory}):
                self.assertEqual(usage.read_login(), ("fake-private-token", "pro"))
                self.assertEqual(path.read_text(), original)
                path.write_text(json.dumps({"claudeAiOauth": {"accessToken": "fake\nheader-injection"}}))
                with self.assertRaises(usage.UsageError):
                    usage.read_login()

    def test_main_never_prints_exception_details(self):
        output = io.StringIO()
        with mock.patch.object(usage, "fetch_usage", side_effect=RuntimeError("Bearer fake-private-token")), \
             contextlib.redirect_stdout(output):
            usage.main()
        self.assertEqual(json.loads(output.getvalue()), {"error": "unavailable"})
        self.assertNotIn("private", output.getvalue())


if __name__ == "__main__":
    unittest.main()
