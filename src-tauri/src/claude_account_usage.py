"""Read subscription usage on the SSH host; never export credentials."""
import datetime
import json
import math
import os
import ssl
import time
import urllib.error
import urllib.request

USAGE_URL = "https://api.anthropic.com/api/oauth/usage"
MAX_RESPONSE_BYTES = 1024 * 1024


class UsageError(Exception):
    def __init__(self, code):
        self.code = code


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        # Never forward the Authorization header through a redirect.
        raise UsageError("redirect")


def reset_milliseconds(value):
    if not isinstance(value, str):
        return None
    try:
        parsed = datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
        if parsed.tzinfo is None:
            return None
        stamp = parsed.timestamp() * 1000
        return int(stamp) if math.isfinite(stamp) and 0 < stamp <= 9007199254740991 else None
    except (ValueError, OverflowError, OSError):
        return None


def normalize_window(value):
    if not isinstance(value, dict):
        return None
    used = value.get("utilization")
    # HTTP utilization is already a percentage, unlike stream event fractions.
    if type(used) not in (int, float) or not math.isfinite(used) or not 0 <= used <= 100:
        return None
    return {"usedPercent": used, "resetsAt": reset_milliseconds(value.get("resets_at"))}


def read_login():
    directory = os.environ.get("CLAUDE_CONFIG_DIR") or os.path.expanduser("~/.claude")
    try:
        with open(os.path.join(directory, ".credentials.json"), "rb") as handle:
            raw = handle.read(MAX_RESPONSE_BYTES + 1)
        if len(raw) > MAX_RESPONSE_BYTES:
            raise UsageError("auth_required")
        login = json.loads(raw).get("claudeAiOauth")
        if not isinstance(login, dict):
            raise UsageError("auth_required")
        token = login.get("accessToken")
        if not isinstance(token, str) or not token or len(token) > 16384 or any(c in token for c in "\r\n"):
            raise UsageError("auth_required")
        plan = login.get("subscriptionType")
        if plan not in ("pro", "max", "team", "enterprise"):
            plan = None
        return token, plan
    except UsageError:
        raise
    except Exception:
        raise UsageError("auth_required") from None


def fetch_usage():
    token, plan = read_login()
    request = urllib.request.Request(
        USAGE_URL,
        method="GET",
        headers={
            "Authorization": "Bearer " + token,
            "anthropic-beta": "oauth-2025-04-20",
            "Accept": "application/json",
            "User-Agent": "CC-GUI-Remote/1.1.4",
        },
    )
    opener = urllib.request.build_opener(
        NoRedirect(), urllib.request.HTTPSHandler(context=ssl.create_default_context())
    )
    try:
        with opener.open(request, timeout=20) as response:
            raw = response.read(MAX_RESPONSE_BYTES + 1)
        if len(raw) > MAX_RESPONSE_BYTES:
            raise UsageError("response_invalid")
        data = json.loads(raw)
        if not isinstance(data, dict):
            raise UsageError("response_invalid")
        return {
            "plan": plan,
            "fiveHour": normalize_window(data.get("five_hour")),
            "sevenDay": normalize_window(data.get("seven_day")),
            "updatedAt": time.time_ns() // 1000000,
        }
    except UsageError:
        raise
    except urllib.error.HTTPError as error:
        # Do not read or return the response body, headers, token, or URL.
        codes = {401: "auth_expired", 403: "forbidden", 429: "rate_limited"}
        raise UsageError(codes.get(error.code, "http_error")) from None
    except (ValueError, UnicodeError):
        raise UsageError("response_invalid") from None
    except Exception:
        raise UsageError("network_error") from None


def main():
    try:
        result = fetch_usage()
    except UsageError as error:
        result = {"error": error.code}
    except Exception:
        result = {"error": "unavailable"}
    print(json.dumps(result, separators=(",", ":"), allow_nan=False))


if __name__ == "__main__":
    main()
