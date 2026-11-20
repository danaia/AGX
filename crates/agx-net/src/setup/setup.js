"use strict";

const form = document.querySelector("form");
const button = form.querySelector('[type="submit"]');
const result = document.querySelector("#result");
const intro = document.querySelector(".intro");
const success = document.querySelector("#confirmation");
const successHeading = document.querySelector("#confirmation-heading");
const connectionResult = document.querySelector("#connection-result");
const scanStatus = document.querySelector("#scan-status");
const security = form.elements.security;
const wifiPassword = form.elements.wifi_password;
const accountPassword = form.elements.password;
const confirmation = document.querySelector("#password-confirmation");
const scanButton = document.querySelector("#scan");
const networks = document.querySelector("#networks");
const networkName = form.elements.ssid;
const countryCode = form.elements.country;
let scanResults = [];
let selectedNetwork = null;

function updateSecurity() {
  const needsPassword = security.value === "Wpa2" || security.value === "Wpa3";
  wifiPassword.required = needsPassword;
  wifiPassword.disabled = !needsPassword;
}

security.addEventListener("change", updateSecurity);
updateSecurity();
button.disabled = false;
scanButton.disabled = false;

function validatePassword() {
  confirmation.setCustomValidity(confirmation.value && confirmation.value !== accountPassword.value
    ? "The account passwords do not match."
    : "");
}

accountPassword.addEventListener("input", validatePassword);
confirmation.addEventListener("input", validatePassword);

async function submit(path, settings) {
  const response = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(settings),
  });
  return response.json();
}

scanButton.addEventListener("click", async () => {
  if (!countryCode.reportValidity()) return;
  scanButton.disabled = true;
  button.disabled = true;
  scanStatus.dataset.tone = "";
  scanStatus.textContent = "Looking for nearby Wi-Fi networks…";
  try {
    const reply = await submit("/scan", { country: countryCode.value });
    if (!reply.ok) {
      scanStatus.dataset.tone = "error";
      scanStatus.textContent = reply.message;
      return;
    }
    scanResults = reply.networks;
    networks.replaceChildren(new Option("Choose a network", ""));
    for (const [index, network] of scanResults.entries()) {
      const securityName = security.querySelector(`option[value="${network.security}"]`)?.text;
      networks.add(new Option(`${network.name} (${securityName})`, String(index)));
    }
    networks.disabled = scanResults.length === 0;
    scanStatus.textContent = networks.disabled
      ? "No nearby networks found. Scan again or enter the network name yourself."
      : "Choose your Wi-Fi network from the list.";
  } catch {
    scanStatus.dataset.tone = "error";
    scanStatus.textContent = "Could not scan. Check the USB-C cable and try again.";
  } finally {
    scanButton.disabled = false;
    button.disabled = false;
  }
});

networks.addEventListener("change", () => {
  selectedNetwork = networks.value === "" ? null : scanResults[Number(networks.value)];
  if (!selectedNetwork) return;
  networkName.value = selectedNetwork.name;
  form.elements.hidden.checked = false;
  security.value = selectedNetwork.security;
  updateSecurity();
});

networkName.addEventListener("input", () => {
  selectedNetwork = null;
  networks.value = "";
});

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  validatePassword();
  if (!form.reportValidity()) return;
  const settings = Object.fromEntries(new FormData(form));
  settings.hidden = form.elements.hidden.checked;
  settings.wifi_password ??= "";
  if (selectedNetwork) settings.ssid_hex = selectedNetwork.ssid_hex;
  button.disabled = true;
  scanButton.disabled = true;
  result.dataset.tone = "";
  result.textContent = "Connecting to Wi-Fi and checking the Internet connection…";
  try {
    const reply = await submit("/claim", settings);
    if (reply.ok) {
      form.reset();
      form.setAttribute("hidden", "");
      intro.hidden = true;
      result.textContent = "";
      connectionResult.textContent = reply.message;
      success.hidden = false;
      successHeading.focus();
      return;
    }
    result.dataset.tone = "error";
    result.textContent = reply.message;
  } catch {
    result.dataset.tone = "error";
    result.textContent = "The setup connection ended. Check the USB-C cable and reopen this page.";
  } finally {
    button.disabled = form.hasAttribute("hidden");
    scanButton.disabled = form.hasAttribute("hidden");
  }
});
