use std::cell::RefCell;
use std::env;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use gtk::gdk::Display;
use gtk::glib;
use gtk::prelude::*;
use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, Entry, Expander,
    Label, Orientation, ScrolledWindow, Spinner,
};
use serde::{Deserialize, Serialize};

const APP_ID: &str = "org.omarchy.VicPanel";
const DEFAULT_GATEWAY: &str = "http://127.0.0.1:8787";
const DEVICE_ID: &str = "vic-native-panel";

#[derive(Clone, Debug, Deserialize)]
struct Message {
    sequence: i64,
    role: String,
    content: String,
    provider: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ActiveConversation {
    conversation_id: String,
    messages: Vec<Message>,
}

#[derive(Debug, Serialize)]
struct TurnRequest<'a> {
    session_id: Option<&'a str>,
    text: &'a str,
    request_id: String,
}

#[derive(Debug, Deserialize)]
struct TurnResponse {
    session_id: String,
    response_text: String,
    provider: String,
}

enum UiEvent {
    Loaded(Result<ActiveConversation, String>),
    TurnFinished {
        user_text: String,
        result: Result<TurnResponse, String>,
    },
}

struct AppState {
    gateway: String,
    session_id: Option<String>,
    messages: Vec<Message>,
    busy: bool,
}

fn main() -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &Application) {
    install_styles();
    let gateway = env::var("VOICEOS_GATEWAY_URL").unwrap_or_else(|_| DEFAULT_GATEWAY.into());
    let state = Rc::new(RefCell::new(AppState {
        gateway,
        session_id: None,
        messages: Vec::new(),
        busy: false,
    }));
    let (sender, receiver) = mpsc::channel::<UiEvent>();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("VIC Panel · Omarchy Voice")
        .default_width(1280)
        .default_height(820)
        .build();
    window.add_css_class("vic-window");

    let root = GtkBox::new(Orientation::Vertical, 16);
    root.set_margin_top(22);
    root.set_margin_bottom(22);
    root.set_margin_start(24);
    root.set_margin_end(24);

    let header = GtkBox::new(Orientation::Horizontal, 12);
    let title_box = GtkBox::new(Orientation::Vertical, 2);
    let kicker = Label::new(Some("VIC PANEL · OMARCHY VOICE"));
    kicker.add_css_class("kicker");
    kicker.set_halign(Align::Start);
    let title = Label::new(Some("Talk with VIC"));
    title.add_css_class("page-title");
    title.set_halign(Align::Start);
    title_box.append(&kicker);
    title_box.append(&title);
    title_box.set_hexpand(true);
    let status = Label::new(Some("Connecting…"));
    status.add_css_class("status-pill");
    header.append(&title_box);
    header.append(&status);
    root.append(&header);

    let content = GtkBox::new(Orientation::Horizontal, 16);
    content.set_vexpand(true);

    let voice_card = GtkBox::new(Orientation::Vertical, 14);
    voice_card.add_css_class("panel");
    voice_card.add_css_class("voice-card");
    voice_card.set_size_request(340, 340);
    voice_card.set_valign(Align::Start);
    let voice_kicker = Label::new(Some("VOICE CHANNEL READY"));
    voice_kicker.add_css_class("kicker");
    voice_kicker.set_halign(Align::Start);
    let voice_title = Label::new(Some("What can I help with?"));
    voice_title.add_css_class("voice-title");
    voice_title.set_wrap(true);
    voice_title.set_halign(Align::Start);
    let voice_hint = Label::new(Some("Type naturally below. Native microphone and push-to-talk are the next milestone."));
    voice_hint.add_css_class("muted");
    voice_hint.set_wrap(true);
    voice_hint.set_halign(Align::Start);
    let orb = Button::with_label("VIC\nREADY");
    orb.add_css_class("vic-orb");
    orb.set_halign(Align::Center);
    orb.set_valign(Align::Center);
    voice_card.append(&voice_kicker);
    voice_card.append(&voice_title);
    voice_card.append(&voice_hint);
    voice_card.append(&orb);
    content.append(&voice_card);

    let conversation_card = GtkBox::new(Orientation::Vertical, 12);
    conversation_card.add_css_class("panel");
    conversation_card.add_css_class("conversation-card");
    conversation_card.set_hexpand(true);
    conversation_card.set_vexpand(true);
    let conversation_header = GtkBox::new(Orientation::Horizontal, 8);
    let conversation_title = Label::new(Some("Current thread"));
    conversation_title.add_css_class("section-title");
    conversation_title.set_hexpand(true);
    conversation_title.set_halign(Align::Start);
    let memory = Label::new(Some("MEMORY ACTIVE"));
    memory.add_css_class("memory-pill");
    conversation_header.append(&conversation_title);
    conversation_header.append(&memory);
    conversation_card.append(&conversation_header);

    let message_list = GtkBox::new(Orientation::Vertical, 9);
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&message_list)
        .build();
    conversation_card.append(&scroll);

    let composer = GtkBox::new(Orientation::Horizontal, 8);
    let entry = Entry::builder().placeholder_text("Ask VIC…").hexpand(true).build();
    let send = Button::with_label("Send");
    send.add_css_class("send-button");
    let spinner = Spinner::new();
    spinner.set_visible(false);
    composer.append(&entry);
    composer.append(&spinner);
    composer.append(&send);
    conversation_card.append(&composer);
    content.append(&conversation_card);
    root.append(&content);
    window.set_child(Some(&root));

    let submit = {
        let entry = entry.clone();
        let state = state.clone();
        let sender = sender.clone();
        let status = status.clone();
        let spinner = spinner.clone();
        move || {
            let text = entry.text().trim().to_owned();
            if text.is_empty() || state.borrow().busy {
                return;
            }
            entry.set_text("");
            state.borrow_mut().busy = true;
            status.set_text("VIC IS THINKING");
            spinner.set_visible(true);
            spinner.start();
            let gateway = state.borrow().gateway.clone();
            let session = state.borrow().session_id.clone();
            let tx = sender.clone();
            thread::spawn(move || {
                let result = send_turn(&gateway, session.as_deref(), &text);
                let _ = tx.send(UiEvent::TurnFinished { user_text: text, result });
            });
        }
    };
    let submit_button = submit.clone();
    send.connect_clicked(move |_| submit_button());
    entry.connect_activate(move |_| submit());

    let event_state = state.clone();
    let event_status = status.clone();
    let event_spinner = spinner.clone();
    let event_entry = entry.clone();
    glib::timeout_add_local(Duration::from_millis(80), move || {
        while let Ok(event) = receiver.try_recv() {
            match event {
                UiEvent::Loaded(Ok(active)) => {
                    let mut current = event_state.borrow_mut();
                    current.session_id = Some(active.conversation_id);
                    current.messages = active.messages;
                    event_status.set_text("ONLINE");
                    render_messages(&message_list, &current.messages);
                }
                UiEvent::Loaded(Err(error)) => {
                    event_status.set_text(&format!("OFFLINE · {error}"));
                }
                UiEvent::TurnFinished { user_text, result } => {
                    let mut current = event_state.borrow_mut();
                    current.busy = false;
                    event_spinner.stop();
                    event_spinner.set_visible(false);
                    event_entry.grab_focus();
                    match result {
                        Ok(turn) => {
                            let next = current.messages.last().map_or(1, |message| message.sequence + 1);
                            current.messages.push(Message { sequence: next, role: "user".into(), content: user_text, provider: None });
                            current.messages.push(Message { sequence: next + 1, role: "assistant".into(), content: turn.response_text, provider: Some(turn.provider) });
                            current.session_id = Some(turn.session_id);
                            event_status.set_text("ONLINE");
                            render_messages(&message_list, &current.messages);
                        }
                        Err(error) => event_status.set_text(&format!("ERROR · {error}")),
                    }
                }
            }
        }
        glib::ControlFlow::Continue
    });

    let gateway = state.borrow().gateway.clone();
    thread::spawn(move || {
        let _ = sender.send(UiEvent::Loaded(load_conversation(&gateway)));
    });
    window.present();
}

fn render_messages(container: &GtkBox, messages: &[Message]) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    let latest_assistant = messages.iter().rposition(|message| message.role == "assistant");
    for (index, message) in messages.iter().enumerate() {
        if message.role != "user" && message.role != "assistant" {
            continue;
        }
        let card = GtkBox::new(Orientation::Vertical, 5);
        card.add_css_class("message");
        card.add_css_class(if message.role == "user" { "message-user" } else { "message-vic" });
        let role = if message.role == "user" { "YOU" } else { "VIC" };
        let meta = message.provider.as_deref().unwrap_or("");
        let heading = Label::new(Some(&format!("{role}  {meta}")));
        heading.add_css_class("message-role");
        heading.set_halign(Align::Start);
        card.append(&heading);
        if message.role == "assistant" && Some(index) != latest_assistant {
            let expander = Expander::new(Some(&first_sentence(&message.content)));
            let full = message_label(&message.content);
            expander.set_child(Some(&full));
            card.append(&expander);
        } else {
            card.append(&message_label(&message.content));
        }
        container.append(&card);
    }
}

fn message_label(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.set_halign(Align::Start);
    label.set_xalign(0.0);
    label
}

fn first_sentence(text: &str) -> String {
    let trimmed = text.trim();
    for (index, character) in trimmed.char_indices() {
        if matches!(character, '.' | '!' | '?') {
            return trimmed[..=index].to_owned();
        }
    }
    trimmed.chars().take(180).collect()
}

fn load_conversation(gateway: &str) -> Result<ActiveConversation, String> {
    let mut response = ureq::get(format!("{gateway}/v1/conversations/active"))
        .header("X-VoiceOS-Device-ID", DEVICE_ID)
        .call()
        .map_err(|error| error.to_string())?;
    response.body_mut().read_json().map_err(|error| error.to_string())
}

fn send_turn(gateway: &str, session_id: Option<&str>, text: &str) -> Result<TurnResponse, String> {
    let request = TurnRequest {
        session_id,
        text,
        request_id: format!("vic-native-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()),
    };
    let mut response = ureq::post(format!("{gateway}/v1/turns/text"))
        .header("X-VoiceOS-Device-ID", DEVICE_ID)
        .send_json(&request)
        .map_err(|error| error.to_string())?;
    response.body_mut().read_json().map_err(|error| error.to_string())
}

fn install_styles() {
    let provider = CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    if let Some(display) = Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::first_sentence;

    #[test]
    fn summary_uses_first_sentence() {
        assert_eq!(first_sentence("First result. More detail."), "First result.");
    }
}
