"use strict";

const $ = (id) => document.getElementById(id);
const fragment = new URLSearchParams(location.hash.slice(1));
const entry = fragment.get("id");
const token = fragment.get("key") || sessionStorage.getItem("angi-editor-key");
if (fragment.has("key") && token) {
    sessionStorage.setItem("angi-editor-key", token);
    fragment.delete("key");
    history.replaceState(null, "", "#" + fragment);
}
let record, editor, plain, file, original, modified, savedVersion, busy = false, recorded = false;

async function api(action, fields = {}) {
    const response = await fetch("/api/" + action, {
        method: "POST",
        headers: { "Content-Type": "application/json", "X-ANGI-Editor": token || "" },
        body: JSON.stringify({ id: entry, ...fields }),
    });
    if (!response.ok) throw new Error("Reopen this record from Agent Angi.");
    const reply = await response.json();
    if (reply.error) throw new Error(reply.error);
    return reply;
}

function dirty() {
    return !recorded && Boolean(modified && file?.revision && modified.getAlternativeVersionId() !== savedVersion);
}

function controls() {
    $("save").disabled = busy || !dirty();
    $("refresh").disabled = busy || !record;
    $("compare").disabled = busy || !modified || !file?.revision || (!file.patch && file.original === null) || dirty();
    $("compare").textContent = recorded ? "Edit working copy" : file?.patch ? "Recorded patch" : "Recorded diff";
}

function status(message) {
    $("status").textContent = message;
}

function placeholder(message) {
    $("editor").hidden = true;
    $("plain-editor").hidden = true;
    $("placeholder").hidden = false;
    $("placeholder").textContent = message;
}

function clearEditors() {
    editor.setModel(null);
    plain.setModel(null);
    original?.dispose();
    modified?.dispose();
    original = modified = null;
}

function showDiff() {
    clearEditors();
    const hasDiff = file.original !== null && file.recorded !== null;
    const text = recorded ? (hasDiff ? file.recorded : file.patch) : file.text;
    if (typeof text !== "string" || (!hasDiff && recorded && !text)) {
        placeholder("No text diff is available for this file.");
        status("");
        return;
    }
    const uri = monaco.Uri.from({ scheme: "angi-work", path: "/" + file.path });
    const readOnly = recorded || !file.revision;
    const patch = !hasDiff && recorded;
    modified = monaco.editor.createModel(text, patch ? "angi-patch" : undefined, uri);
    savedVersion = modified.getAlternativeVersionId();
    modified.onDidChangeContent(controls);
    $("placeholder").hidden = true;
    $("editor").hidden = !hasDiff;
    $("plain-editor").hidden = hasDiff;
    if (hasDiff) {
        original = monaco.editor.createModel(file.original, modified.getLanguageId(), uri.with({ scheme: "angi-before" }));
        editor.updateOptions({ readOnly });
        editor.setModel({ original, modified });
        editor.layout();
    } else {
        plain.updateOptions({ readOnly });
        plain.setModel(modified);
        plain.layout();
    }
    if (recorded) {
        status(file.text === null ? "Recorded changes · working copy unavailable" : "Recorded changes");
    } else {
        status(hasDiff ? "Working copy · compared with the start of this run"
            : file.patch ? "Working copy · open the recorded patch to review agent changes"
            : "Working copy · recorded text diff unavailable");
    }
}

async function selectFile(item, button) {
    if (busy || (dirty() && !confirm("Discard the unsaved changes in this file?"))) return;
    busy = true;
    controls();
    try {
        const data = await api("file", { path: item.path });
        file = { ...data, path: item.path };
        recorded = file.text === null || Boolean(file.patch);
        $("editor-title").textContent = item.path.split("/").at(-1);
        showDiff();
    } catch (error) {
        clearEditors();
        file = null;
        placeholder(error.message);
        status("");
    } finally {
        document.querySelector("#files [aria-current]")?.removeAttribute("aria-current");
        button.setAttribute("aria-current", "true");
        $("location").textContent = record.root + (item.path.includes("/") ? "/" + item.path.split("/").slice(0, -1).join("/") : "");
        $("folder").onclick = () => call("folder", { path: item.path });
        busy = false;
        controls();
    }
}

async function save() {
    if (busy || !dirty()) return;
    busy = true;
    controls();
    const text = modified.getValue(monaco.editor.EndOfLinePreference.TextDefined, true);
    const version = modified.getAlternativeVersionId();
    try {
        const result = await api("save", { path: file.path, text, revision: file.revision });
        file.text = text;
        file.revision = result.revision;
        savedVersion = version;
        status("Saved to disk.");
    } catch (error) {
        status(error.message);
    } finally {
        busy = false;
        controls();
    }
}

async function call(action, fields) {
    try {
        await api(action, fields);
    } catch (error) {
        status(error.message);
    }
}

function showRecord() {
    $("mode").textContent = record.mode === "os" ? "OS update" : "Project";
    $("work-title").textContent = record.title;
    $("brief").textContent = record.brief;
    $("summary-label").textContent = record.summary_label || "Outcome";
    $("summary").textContent = record.summary || "No outcome was recorded for this run.";
    $("location").textContent = record.root || record.status;
    $("folder").disabled = !record.root;
    if (record.mode === "os") $("refresh").title = "Reload the installed ANGI desktop";
    $("folder").onclick = () => call("folder", { path: record.root });
    for (const resource of record.resources) {
        const button = document.createElement("button");
        button.textContent = resource.label + " ↗";
        button.title = resource.path;
        button.onclick = () => call("folder", { path: resource.path });
        $("resources").append(button);
    }
    $("record-note").textContent = record.status;
    $("record-note").hidden = !record.status;
    controls();
}

function showFiles() {
    for (const item of record.files) {
        const row = document.createElement("li");
        const button = document.createElement("button");
        const path = document.createElement("span");
        const kind = document.createElement("span");
        path.textContent = item.path;
        kind.textContent = item.kind;
        kind.className = "kind " + item.kind.toLowerCase();
        button.append(path, kind);
        row.append(button);
        $("files").append(row);
        button.onclick = () => selectFile(item, button);
    }
    if (record.files.length) $("files").querySelector("button").click();
    else placeholder(record.status || "No files changed during this run.");
}

$("save").onclick = save;
$("refresh").onclick = () => {
    if (dirty()) {
        status("Save your changes before refreshing the desktop.");
        return;
    }
    if (record.mode === "os") status("Refresh reloads the installed desktop. Install source changes through Services.");
    call("refresh");
};
$("compare").onclick = () => {
    if (busy || dirty() || !file?.revision) return;
    recorded = !recorded;
    showDiff();
    controls();
};
window.addEventListener("beforeunload", (event) => {
    if (dirty()) {
        event.preventDefault();
        event.returnValue = "";
    }
});

async function start() {
    try {
        record = await api("record");
        showRecord();
        window.MonacoEnvironment = { getWorkerUrl: () => "/worker.js" };
        require.config({ paths: { vs: "vs" } });
        require(["vs/editor/editor.main"], () => {
            editor = monaco.editor.createDiffEditor($("editor"), {
                theme: "vs-dark", originalEditable: false, automaticLayout: true,
                renderSideBySide: false, minimap: { enabled: false }, scrollBeyondLastLine: false,
                wordWrap: "on", diffWordWrap: "on", renderOverviewRuler: false,
            });
            plain = monaco.editor.create($("plain-editor"), {
                theme: "vs-dark", automaticLayout: true, minimap: { enabled: false },
                scrollBeyondLastLine: false, wordWrap: "on", model: null,
            });
            monaco.languages.register({ id: "angi-patch" });
            monaco.languages.setMonarchTokensProvider("angi-patch", {
                tokenizer: { root: [
                    [/^@@.*$/, "keyword"],
                    [/^\+.*$/, "string"],
                    [/^-.*$/, "invalid"],
                ] },
            });
            for (const input of [editor.getModifiedEditor(), plain]) {
                input.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, save);
            }
            showFiles();
        }, () => placeholder("Monaco Editor could not load."));
    } catch (error) {
        $("work-title").textContent = "Work record unavailable";
        placeholder(error.message);
    }
}
start();
