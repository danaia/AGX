"""Private OpenCode provider bridge; credentials never leave OpenCode's store."""
import base64
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import threading
import re
import time
import urllib.error
import urllib.request
from urllib.parse import urlsplit

import tailnet
from messaging import ROLES
from models import ALL_ROLES, activate, active, atomic_write, save, selections


class Providers:
    def __init__(self):
        self.process = None
        self.url = None
        self.password = secrets.token_urlsafe(32)
        self.pending = False
        self.message = ""
        self.tailscale_waiting = False

    def headers(self):
        return {"Authorization": "Basic " + base64.b64encode(("opencode:" + self.password).encode()).decode()}

    def wait_until_serving(self, deadline=60):
        """OpenCode opens its port before it can serve; a request sent in that window is accepted and never answered.

        Probe with short-lived requests on fresh connections until one gets any HTTP response.
        """
        probe = urllib.request.Request(self.url + "/global/health", headers=self.headers())
        end = time.monotonic() + deadline
        while time.monotonic() < end:
            if self.process.poll() is not None:
                raise RuntimeError("OpenCode provider service could not start.")
            try:
                with urllib.request.urlopen(probe, timeout=1.5):
                    return
            except urllib.error.HTTPError:
                return
            except OSError:
                time.sleep(.2)
        raise RuntimeError("OpenCode provider service did not become ready. Try again.")

    def api(self, path, data=None, timeout=30):
        if self.process is not None and self.process.poll() is not None:
            self.process = None
        if self.process is None:
            with socket.socket() as listener:
                listener.bind(("127.0.0.1", 0))
                port = listener.getsockname()[1]
            self.url = f"http://127.0.0.1:{port}"
            self.process = subprocess.Popen(["opencode", "serve", "--hostname", "127.0.0.1", "--port", str(port)], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env={**os.environ, "OPENCODE_SERVER_PASSWORD": self.password})
            try:
                self.wait_until_serving()
            except BaseException:
                self.close()
                self.process = None
                raise
        request = urllib.request.Request(self.url + path, data=None if data is None else json.dumps(data).encode(), headers={**self.headers(), "Content-Type": "application/json"})
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return json.load(response)

    def status(self, provider):
        if provider == "lmstudio":
            config = lmstudio_config()
            where = tailnet.target()
            if not config:
                text = "Run LM Studio on your computer and reach it privately over Tailscale. Sign in to Tailscale below, then enter your computer's Tailscale name or address."
            elif active() == "lmstudio":
                text = "In use. " + ("Changes apply to new builds." if selections("lmstudio") else "Choose a model for each role; until you do, new builds use OpenCode's default model.")
            else:
                text = "Connected. Use it to make it the one service Agent Angi builds with."
            if config and where:
                text += f" LM Studio is at {where[0]}:{where[1]}."
            network = tailnet.state()
            if network["ready"]:
                self.tailscale_waiting = False
            elif self.tailscale_waiting and network["login"]:
                network["text"] = "Waiting for you to approve the sign-in in the browser window…"
            return {"connected": bool(config), "active": active() == "lmstudio" and bool(config), "models": ["lmstudio/" + key for key in config.get("models", {})], "status": text, "choices": selections("lmstudio"), "tailscale": network}
        if provider == "apple":
            config = apple_config()
            return {"connected": bool(config), "active": active() == "apple", "models": ["apple/" + key for key in config.get("models", {})], "status": "Server configured. A role with no model chosen uses Apple's too. Test the connection again if the device address changes." if config else "Install Local LLM Server on your Apple Intelligence device, enable Apple Intelligence, start its OpenAI-compatible server and enter the address shown by the app. Keep both devices on the same trusted network and keep the app running.", "choices": apple_models(), "address": apple_address(config)}
        connected = signed_in(provider)
        models = []
        if connected:
            catalog = self.api("/provider")
            for item in catalog["all"]:
                if item["id"] == provider:
                    models = sorted(provider + "/" + key for key, model in item["models"].items() if model.get("capabilities", {}).get("toolcall", True) and model.get("status") != "deprecated")
        in_use = connected and active() == provider
        chosen = "Changes apply to new builds." if selections(provider) else "Choose a model for each role; until you do, new builds use OpenCode's default model."
        notice = "In use. " + chosen if in_use else "Connected. Use it to make it the one service Agent Angi builds with."
        return {"connected": connected, "active": in_use, "models": models, "choices": selections(provider), "status": notice if connected else (self.message or ("Complete sign-in in the browser, then return here." if self.pending else "Sign in with your ChatGPT subscription to connect OpenAI to OpenCode."))}

    def use(self, provider):
        if not self.status(provider)["connected"]:
            raise ValueError("Connect this service before using it.")
        activate(provider)
        return self.status(provider)

    def connect(self):
        if self.pending:
            raise ValueError("A sign-in is already in progress. Complete it in the browser first.")
        methods = self.api("/provider/auth", timeout=90).get("openai", [])
        index = next((i for i, method in enumerate(methods) if method["type"] == "oauth" and "browser" in method["label"].lower()), None)
        if index is None:
            raise ValueError("This OpenCode runtime has no OpenAI browser sign-in method.")
        result = self.api("/provider/openai/oauth/authorize", {"method": index})
        if result["method"] != "auto":
            raise ValueError("This provider requires a manual authorization code.")
        self.pending = True
        self.message = ""
        def finish():
            try:
                self.api("/provider/openai/oauth/callback", {"method": index}, timeout=900)
            except Exception:
                self.message = "Sign-in did not complete. Try connecting again."
            finally:
                self.pending = False
        threading.Thread(target=finish, daemon=True).start()
        return {"url": result["url"], "status": result.get("instructions", "Complete sign-in in your browser.")}

    def close(self):
        if self.process is not None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()


def signed_in(provider):
    credentials = Path(os.environ["XDG_DATA_HOME"]) / "opencode/auth.json"
    return provider in (json.loads(credentials.read_text()) if credentials.exists() else {})


def overview():
    """Connection and use of every card, from local files only so it never starts OpenCode."""
    current = active()
    ready = {"apple": bool(apple_config()), "openai": signed_in("openai"), "lmstudio": bool(lmstudio_config())}
    return {name: {"connected": connected, "active": connected and name == current, "models": ((apple_models() if name == "apple" else selections(name)) if connected else {})} for name, connected in ready.items()}


def local_config(service):
    path = Path(os.environ["XDG_CONFIG_HOME"]) / f"opencode/angi-{service}.json"
    return json.loads(path.read_text()) if path.exists() else {}


def apple_config():
    return local_config("apple")


def apple_address_path():
    return Path(os.environ["XDG_STATE_HOME"]) / "angi-swarm/apple-address.json"


def apple_address(config):
    try:
        saved = json.loads(apple_address_path().read_text())
        if isinstance(saved, str) and saved:
            return saved
    except (FileNotFoundError, ValueError):
        pass
    return config.get("options", {}).get("baseURL", "")


def apple_models():
    """Each role's Apple model as a run will use it: the saved choice, or the Apple model it falls back to."""
    import applefit
    return applefit.effective(selections("apple"), apple_config(), ROLES, ALL_ROLES)


def lmstudio_config():
    return local_config("lmstudio")


def parse_lmstudio(address):
    """Host and port from what a person types: a Tailscale name, an address, or a pasted server URL."""
    text = address.strip()
    parsed = urlsplit(text if "://" in text else "http://" + text)
    try:
        port = parsed.port or 1234
    except ValueError:
        raise ValueError("Enter the port as a number, for example your-computer:1234.")
    host = parsed.hostname or ""
    if parsed.scheme != "http" or parsed.username or parsed.password or parsed.query or parsed.fragment or parsed.path not in ("", "/", "/v1", "/v1/"):
        raise ValueError("Enter your computer's Tailscale name or address, for example your-computer:1234.")
    if not re.fullmatch(r"[A-Za-z0-9._-]+", host) or host.startswith("-"):
        raise ValueError("Enter your computer's Tailscale name or address, for example your-computer:1234.")
    return host, port


def connect_lmstudio(address):
    host, port = parse_lmstudio(address)
    if not tailnet.state()["ready"]:
        raise ValueError("Sign in to Tailscale first.")
    previous = tailnet.target()
    tailnet.set_target(host, port)
    try:
        # The test uses the same loopback relay as builds do, so it proves the whole path.
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with opener.open(tailnet.RELAY + "/models", timeout=20) as response:
            data = json.load(response)
    except (OSError, ValueError):
        if previous:
            tailnet.set_target(*previous)
        raise ValueError(f"Could not reach LM Studio at {host}:{port} over Tailscale. In LM Studio, load a model, start the server and turn on Serve on local network, then check the address.")
    identifiers = [item["id"] for item in data.get("data", []) if isinstance(item.get("id"), str) and item["id"] and "embed" not in item["id"].lower()]
    if not identifiers:
        if previous:
            tailnet.set_target(*previous)
        raise ValueError("LM Studio returned no chat models. Load a model in LM Studio first.")
    config = {"npm": "@ai-sdk/openai-compatible", "name": "LM Studio", "options": {"baseURL": tailnet.RELAY}, "models": {key: {"name": key} for key in identifiers}}
    path = Path(os.environ["XDG_CONFIG_HOME"]) / "opencode/angi-lmstudio.json"
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    atomic_write(path, config)
    return config


def connect_apple(address):
    address = address.strip()
    parsed = urlsplit(address)
    if parsed.scheme not in ("http", "https") or not parsed.hostname or parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise ValueError("Enter the server's HTTP or HTTPS address, without credentials.")
    try:
        parsed.port
    except ValueError:
        raise ValueError("Enter a valid server port.") from None
    if any(character.isspace() for character in address):
        raise ValueError("Enter the server address without spaces.")
    atomic_write(apple_address_path(), address)
    address = address.rstrip("/")
    if not address.endswith("/v1"):
        address += "/v1"
    with urllib.request.urlopen(address + "/models", timeout=10) as response:
        data = json.load(response)
    identifiers = [item["id"] for item in data["data"] if isinstance(item.get("id"), str) and item["id"]]
    if not identifiers:
        raise ValueError("The server returned no models.")
    config = {"npm": "@ai-sdk/openai-compatible", "name": "Apple Intelligence", "options": {"baseURL": address}, "models": {key: {"name": key} for key in identifiers}}
    path = Path(os.environ["XDG_CONFIG_HOME"]) / "opencode/angi-apple.json"
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    atomic_write(path, config)
    return config


def bridge():
    providers = Providers()
    try:
        for line in sys.stdin:
            try:
                request = json.loads(line)
                provider = request.get("provider", "openai")
                if provider not in ("apple", "openai", "lmstudio"):
                    raise ValueError("Unknown provider.")
                action = request["action"]
                if action == "connect":
                    result = providers.connect()
                elif action == "apple":
                    connect_apple(request["address"])
                    result = providers.status("apple")
                elif action == "lmstudio":
                    connect_lmstudio(request["address"])
                    result = providers.status("lmstudio")
                elif action == "tailscale":
                    url = tailnet.login()
                    result = providers.status("lmstudio")
                    if url:
                        providers.tailscale_waiting = True
                        result["tailscale"]["text"] = "Waiting for you to approve the sign-in in the browser window…"
                        result.update({"url": url, "status": "Approve the sign-in in the browser window, then return here."})
                elif action == "save":
                    save(request["role"], request["model"])
                    result = providers.status(provider)
                elif action == "status":
                    result = providers.status(provider)
                elif action == "activate":
                    result = providers.use(provider)
                elif action == "overview":
                    result = {}
                else:
                    raise ValueError("Unknown provider action.")
            except Exception as error:
                result = {"error": str(error)[:240] if isinstance(error, (ValueError, RuntimeError)) else "Connection failed. Check the server address and network, then try again."}
            try:
                result["overview"] = overview()
            except Exception:
                pass
            print(json.dumps(result), flush=True)
    finally:
        providers.close()
