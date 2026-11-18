//! One pending question at a time for the person at the desktop, with a bounded wait.
use agx_bluetooth::{PROMPT_SECONDS, Prompt, PromptKind, valid_pin};
use std::{
    future::Future,
    sync::Mutex,
    time::Duration,
};
use tokio::sync::oneshot;

#[derive(Debug, PartialEq, Eq)]
pub struct Answer {
    pub accept: bool,
    pub value: Option<String>,
}
impl Answer {
    const REJECTED: Self = Self { accept: false, value: None };
}

struct Pending {
    prompt: Prompt,
    reply: Option<oneshot::Sender<Answer>>,
}

#[derive(Default)]
pub struct Prompter {
    next: Mutex<u32>,
    pending: Mutex<Option<Pending>>,
}

impl Prompter {
    pub fn current(&self) -> Option<Prompt> {
        self.pending.lock().unwrap().as_ref().map(|pending| pending.prompt.clone())
    }

    fn install(&self, kind: PromptKind, address: &str, name: &str, code: Option<String>, reply: Option<oneshot::Sender<Answer>>) -> u32 {
        let id = {
            let mut next = self.next.lock().unwrap();
            *next = next.wrapping_add(1).max(1);
            *next
        };
        let prompt = Prompt { id, kind, address: address.into(), name: name.into(), code };
        // BlueZ pairs one device at a time; a newer question replaces (and so refuses) an older one.
        if let Some(Pending { reply: Some(old), .. }) = self.pending.lock().unwrap().replace(Pending { prompt, reply }) {
            let _ = old.send(Answer::REJECTED);
        }
        id
    }

    fn clear(&self, id: u32) {
        let mut pending = self.pending.lock().unwrap();
        if pending.as_ref().is_some_and(|p| p.prompt.id == id) {
            *pending = None;
        }
    }

    /// Ask and wait. Silence, a replaced prompt or a dropped service all count as refusal.
    pub async fn ask(&self, kind: PromptKind, address: &str, name: &str, code: Option<String>) -> Answer {
        let (send, receive) = oneshot::channel();
        let id = self.install(kind, address, name, code, Some(send));
        let answer = tokio::time::timeout(Duration::from_secs(PROMPT_SECONDS.into()), receive).await;
        self.clear(id);
        answer.ok().and_then(Result::ok).unwrap_or(Answer::REJECTED)
    }

    /// Show a code to type on the device until BlueZ says the attempt ended.
    pub async fn show(&self, kind: PromptKind, address: &str, name: &str, code: String, ended: impl Future<Output = ()>) {
        let id = self.install(kind, address, name, Some(code), None);
        let _ = tokio::time::timeout(Duration::from_secs(PROMPT_SECONDS.into()), ended).await;
        self.clear(id);
    }

    /// The desktop's reply. Returns false for a stale or unknown prompt id, and for a PIN that
    /// BlueZ could not accept, so the prompt stays open for another try.
    pub fn respond(&self, id: u32, accept: bool, value: Option<String>) -> bool {
        let mut pending = self.pending.lock().unwrap();
        let Some(current) = pending.as_mut().filter(|p| p.prompt.id == id) else {
            return false;
        };
        let needs_value = matches!(current.prompt.kind, PromptKind::Pin | PromptKind::Passkey);
        if accept && needs_value {
            let valid = value.as_deref().is_some_and(|text| {
                valid_pin(text) && (current.prompt.kind == PromptKind::Pin || (text.len() == 6 && text.bytes().all(|b| b.is_ascii_digit())))
            });
            if !valid {
                return false;
            }
        }
        match current.reply.take() {
            Some(reply) => {
                *pending = None;
                reply.send(Answer { accept, value: value.filter(|_| accept && needs_value) }).is_ok()
            }
            // A display-only prompt has nothing to answer; dismissing it just closes the card.
            None => {
                *pending = None;
                true
            }
        }
    }
}
