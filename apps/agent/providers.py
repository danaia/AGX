"""Private OpenCode provider bridge; credentials never leave OpenCode's store."""
import base64
import http.client
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
from edition import zero2w
from messaging import ROLES
from models import ALL_ROLES, activate, active, atomic_write, save, selections


class Providers:
    def __init__(self):
        self.process = None
        self.url = None
        self.password = secrets.token_urlsafe(32)
        self.pending = False
        self.device_auth = {}
        self.message = ""
        self.tailscale_waiting = False
        self.apple_health = ("", False)

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
            if zero2w():
                configured = bool(config)
                return {
                    "connected": configured,
                    "active": configured and active() == "lmstudio",
                    "models": ["lmstudio/" + key for key in config.get("models", {})],
                    "status": f'Configured server: {config["options"]["baseURL"]}' if configured else
                              "Connect to LM Studio on your local network.",
                    "choices": selections("lmstudio"),
                }
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
            endpoint = apple_endpoint(config)
            try:
                connected = bool(endpoint and apple_model_ids(endpoint, timeout=2))
            except (OSError, ValueError, http.client.HTTPException):
                connected = False
            self.apple_health = (endpoint, connected)
            return self.apple_status(config)
        connected = signed_in(provider) and not (zero2w() and self.pending)
        models = []
        if connected:
            catalog = self.api("/provider")
            for item in catalog["all"]:
                if item["id"] == provider:
                    models = sorted(provider + "/" + key for key, model in item["models"].items() if model.get("capabilities", {}).get("toolcall", True) and model.get("status") != "deprecated")
        in_use = connected and active() == provider
        chosen = "Changes apply to new builds." if selections(provider) else "Choose a model for each role; until you do, new builds use OpenCode's default model."
        notice = "In use. " + chosen if in_use else "Connected. Use it to make it the one service Agent Angi builds with."
        return {"connected": connected, "active": in_use, "models": models, "choices": selections(provider),
                "pending": self.pending, "status": notice if connected else (self.message or
                ("Complete sign-in in the browser, then return here." if self.pending else
                 "Sign in with your ChatGPT subscription to connect OpenAI to OpenCode.")), **self.device_auth}

    def apple_status(self, config):
        connected = self.apple_health == (apple_endpoint(config), True)
        if connected:
            message = "Connected to Local LLM Server. Test the connection again if the device address changes."
        elif config:
            message = "Disconnected. Start Local LLM Server on your Apple device and keep both devices on the same network."
        else:
            message = "Install Local LLM Server on your Apple Intelligence device, enable Apple Intelligence, start its OpenAI-compatible server and enter the address shown by the app. Keep both devices on the same trusted network and keep the app running."
        return {"connected": connected, "active": active() == "apple" and bool(config),
                "models": ["apple/" + key for key in config.get("models", {})],
                "status": message, "choices": apple_models(), "address": apple_address(config)}

    def use(self, provider):
        status = self.status(provider)
        if not status["connected"]:
            raise ValueError("Connect this service before using it.")
        activate(provider)
        if provider == "apple":
            return {**status, "active": True}
        return self.status(provider)

    def connect(self):
        if self.pending:
            raise ValueError("A sign-in is already in progress. Complete it in the browser first.")
        methods = self.api("/provider/auth", timeout=90).get("openai", [])
        mode = "headless" if zero2w() else "browser"
        index = next((i for i, method in enumerate(methods)
                      if method["type"] == "oauth" and mode in method["label"].lower()), None)
        if index is None:
            raise ValueError(f"This OpenCode runtime has no OpenAI {mode} sign-in method.")
        result = self.api("/provider/openai/oauth/authorize", {"method": index})
        if result["method"] != "auto":
            raise ValueError("This provider requires a manual authorization code.")
        self.pending = True
        self.message = ""
        instructions = result.get("instructions", "Complete sign-in in your browser.")
        if mode == "headless":
            instructions = "Open the address below in your workstation browser. " + instructions
        sign_in = {"url": result["url"], "status": instructions, "pending": True}
        if mode == "headless":
            self.device_auth = sign_in

        def finish():
            try:
                self.api("/provider/openai/oauth/callback", {"method": index}, timeout=900)
            except Exception:
                self.message = "Sign-in did not complete. Try connecting again."
            finally:
                if mode == "headless":
                    self.close()
                    self.device_auth = {}
                self.pending = False
        threading.Thread(target=finish, daemon=True).start()
        return sign_in

    def close(self):
        process, self.process = self.process, None
        if process is not None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


def signed_in(provider):
    credentials = Path(os.environ["XDG_DATA_HOME"]) / "opencode/auth.json"
    return provider in (json.loads(credentials.read_text()) if credentials.exists() else {})


def overview(apple_health=("", False)):
    """Summarize the cards without making network requests or starting OpenCode."""
    current = active()
    config = apple_config()
    ready = {"openai": signed_in("openai"), "lmstudio": bool(lmstudio_config())}
    cards = {name: {"connected": connected, "active": connected and name == current,
                    "models": selections(name) if connected else {}} for name, connected in ready.items()}
    cards["apple"] = {"connected": apple_health == (apple_endpoint(config), True),
                      "configured": bool(config), "active": bool(config) and current == "apple",
                      "models": apple_models() if config else {}}
    return cards


def local_config(service):
    path = Path(os.environ["XDG_CONFIG_HOME"]) / f"opencode/angi-{service}.json"
    return json.loads(path.read_text()) if path.exists() else {}


def apple_config():
    return local_config("apple")


def apple_endpoint(config):
    return config.get("options", {}).get("baseURL", "")


def apple_address_path():
    return Path(os.environ["XDG_STATE_HOME"]) / "angi-swarm/apple-address.json"


def apple_address(config):
    try:
        saved = json.loads(apple_address_path().read_text())
        if isinstance(saved, str) and saved:
            return saved
    except (FileNotFoundError, ValueError):
        pass
    return apple_endpoint(config)


def apple_models():
    """Each role's Apple model as a run will use it: the saved choice, or the Apple model it falls back to."""
    import applefit
    return applefit.effective(selections("apple"), apple_config(), ROLES, ALL_ROLES)


def lmstudio_config():
    return local_config("lmstudio")


def parse_lmstudio(address, *, network="Tailscale"):
    """Host and port from what a person types: a Tailscale name, an address, or a pasted server URL."""
    text = address.strip()
    parsed = urlsplit(text if "://" in text else "http://" + text)
    try:
        port = parsed.port or 1234
    except ValueError:
        raise ValueError("Enter the port as a number, for example your-computer:1234.")
    host = parsed.hostname or ""
    if parsed.scheme != "http" or parsed.username or parsed.password or parsed.query or parsed.fragment or parsed.path not in ("", "/", "/v1", "/v1/"):
        raise ValueError(f"Enter your computer's {network} name or address, for example your-computer:1234.")
    if not re.fullmatch(r"[A-Za-z0-9._-]+", host) or host.startswith("-"):
        raise ValueError(f"Enter your computer's {network} name or address, for example your-computer:1234.")
    return host, port


def connect_lmstudio(address):
    if zero2w():
        host, port = parse_lmstudio(address, network="LAN")
        endpoint = f"http://{host}:{port}/v1"
        try:
            identifiers = lmstudio_models(endpoint)
        except (OSError, ValueError, http.client.HTTPException) as error:
            raise ValueError(f"Could not reach LM Studio at {host}:{port}. Check that its server is running.") from error
        return save_lmstudio(endpoint, identifiers)

    host, port = parse_lmstudio(address)
    if not tailnet.state()["ready"]:
        raise ValueError("Sign in to Tailscale first.")
    previous = tailnet.target()
    tailnet.set_target(host, port)
    try:
        identifiers = lmstudio_models(tailnet.RELAY)
    except (OSError, ValueError):
        if previous:
            tailnet.set_target(*previous)
        raise ValueError(f"Could not reach LM Studio at {host}:{port} over Tailscale. In LM Studio, load a model, start the server and turn on Serve on local network, then check the address.")
    if not identifiers:
        if previous:
            tailnet.set_target(*previous)
        raise ValueError("LM Studio returned no chat models. Load a model in LM Studio first.")
    return save_lmstudio(tailnet.RELAY, identifiers)


def lmstudio_models(endpoint):
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(endpoint + "/models", timeout=20) as response:
        data = json.load(response)
    models = data.get("data") if isinstance(data, dict) else None
    if not isinstance(models, list):
        raise ValueError("LM Studio returned an invalid model list.")
    return [item["id"] for item in models if isinstance(item, dict) and
            isinstance(item.get("id"), str) and item["id"] and "embed" not in item["id"].lower()]


def save_lmstudio(endpoint, identifiers):
    if not identifiers:
        raise ValueError("LM Studio returned no chat models. Load a model in LM Studio first.")
    config = {"npm": "@ai-sdk/openai-compatible", "name": "LM Studio", "options": {"baseURL": endpoint},
              "models": {key: {"name": key} for key in identifiers}}
    path = Path(os.environ["XDG_CONFIG_HOME"]) / "opencode/angi-lmstudio.json"
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    atomic_write(path, config)
    return config


def apple_model_ids(endpoint, timeout):
    with urllib.request.urlopen(endpoint + "/models", timeout=timeout) as response:
        data = json.load(response)
    items = data.get("data") if isinstance(data, dict) else None
    if not isinstance(items, list):
        return []
    return [item["id"] for item in items if isinstance(item, dict) and isinstance(item.get("id"), str) and item["id"]]


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
    identifiers = apple_model_ids(address, timeout=10)
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
                supported = ("openai", "lmstudio") if zero2w() else ("apple", "openai", "lmstudio")
                if provider not in supported:
                    raise ValueError("Unknown provider.")
                action = request["action"]
                if action == "connect":
                    result = providers.connect()
                elif action == "apple":
                    config = connect_apple(request["address"])
                    providers.apple_health = (apple_endpoint(config), True)
                    result = providers.apple_status(config)
                elif action == "lmstudio":
                    connect_lmstudio(request["address"])
                    result = providers.status("lmstudio")
                elif action == "tailscale" and not zero2w():
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
                result["overview"] = overview(providers.apple_health)
            except Exception:
                pass
            if zero2w() and not providers.pending:
                providers.close()
            print(json.dumps(result), flush=True)
    finally:
        providers.close()
