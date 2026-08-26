use std::cell::RefCell;
use std::env;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use gtk::gdk::Display;
use gtk::glib;
use gtk::prelude::*;
use gtk::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, Entry, Expander,
    FileDialog, Label, Orientation, Picture, ScrolledWindow, Spinner,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod attachment_client;
use attachment_client::Attachment;

const APP_ID: &str = "org.omarchy.VicPanel";
const DEFAULT_GATEWAY: &str = "http://127.0.0.1:8787";
const DEVICE_ID: &str = "vic-native-panel";

#[derive(Clone, Debug, Deserialize)]
struct Message {
    sequence: i64,
    role: String,
    content: String,
    provider: Option<String>,
    #[serde(default)]
    attachments: Vec<Attachment>,
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
    attachment_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TurnResponse {
    session_id: String,
    response_text: String,
    provider: String,
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    transcript: String,
}

enum UiEvent {
    Loaded(Result<ActiveConversation, String>),
    TurnFinished {
        user_text: String,
        result: Result<TurnResponse, String>,
    },
    Dashboard(Result<DashboardUpdate, String>),
    AttachmentUploaded(Result<Attachment, String>),
    MemoryReview(Result<Vec<SleepCycleReport>, String>),
    MemoryAction(Result<(), String>),
}

#[derive(Clone, Debug, Deserialize)]
struct SleepCycleRecord {
    id: String,
    mode: String,
    created_at: String,
}

#[derive(Clone, Debug, Deserialize)]
struct SleepCycleChange {
    id: String,
    detail: String,
    status: String,
    confidence: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
struct SleepCycleReport {
    cycle: SleepCycleRecord,
    changes: Vec<SleepCycleChange>,
}

#[derive(Debug, Deserialize)]
struct SleepCycleListResponse {
    sleep_cycles: Vec<SleepCycleReport>,
}

#[derive(Clone, Debug)]
struct DashboardUpdate {
    cursor: i64,
    activities: Vec<String>,
    workers: Vec<String>,
    tasks: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RecoveryResponse {
    latest_event_id: i64,
    events: Vec<ClientEvent>,
}

#[derive(Debug, Deserialize)]
struct ClientEvent {
    #[serde(rename = "type")]
    event_type: String,
    payload: Value,
}

struct AppState {
    gateway: String,
    session_id: Option<String>,
    messages: Vec<Message>,
    busy: bool,
    recorder: Option<Child>,
    recording_path: Option<PathBuf>,
    pending_attachment: Option<Attachment>,
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
        recorder: None,
        recording_path: None,
        pending_attachment: None,
    }));
    let (sender, receiver) = mpsc::channel::<UiEvent>();

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Touch · VoiceOS")
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
    let voice_hint = Label::new(Some(
        "Type naturally below. Native microphone and push-to-talk are the next milestone.",
    ));
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
    let entry = Entry::builder()
        .placeholder_text("Ask VIC…")
        .hexpand(true)
        .build();
    let send = Button::with_label("Send");
    let attach = Button::with_label("＋ Image");
    let attachment_status = Label::new(None);
    attachment_status.add_css_class("muted-small");
    send.add_css_class("send-button");
    let spinner = Spinner::new();
    spinner.set_visible(false);
    composer.append(&attach);
    composer.append(&entry);
    composer.append(&spinner);
    composer.append(&send);
    conversation_card.append(&composer);
    conversation_card.append(&attachment_status);
    content.append(&conversation_card);

    {
        let window = window.clone();
        let state = state.clone();
        let sender = sender.clone();
        attach.connect_clicked(move |_| {
            let chooser = FileDialog::builder()
                .title("Choose an image")
                .modal(true)
                .build();
            let state = state.clone();
            let sender = sender.clone();
            chooser.open(
                Some(&window),
                None::<&gtk::gio::Cancellable>,
                move |result| {
                    let Some(path) = result.ok().and_then(|file| file.path()) else {
                        return;
                    };
                    let gateway = state.borrow().gateway.clone();
                    let sender = sender.clone();
                    thread::spawn(move || {
                        let _ = sender.send(UiEvent::AttachmentUploaded(
                            attachment_client::upload(&gateway, &path),
                        ));
                    });
                },
            );
        });
    }

    let operations = GtkBox::new(Orientation::Vertical, 14);
    operations.set_size_request(280, -1);
    let memory_review_card = GtkBox::new(Orientation::Vertical, 8);
    memory_review_card.add_css_class("panel");
    memory_review_card.add_css_class("memory-review-card");
    let memory_review_header = GtkBox::new(Orientation::Horizontal, 8);
    let memory_review_title = Label::new(Some("Memory review"));
    memory_review_title.add_css_class("section-title-small");
    memory_review_title.set_hexpand(true);
    memory_review_title.set_halign(Align::Start);
    let scan_memories = Button::with_label("Scan now");
    scan_memories.add_css_class("secondary-button");
    memory_review_header.append(&memory_review_title);
    memory_review_header.append(&scan_memories);
    let memory_review_list = GtkBox::new(Orientation::Vertical, 7);
    memory_review_card.append(&memory_review_header);
    memory_review_card.append(&memory_review_list);
    let activity_card = GtkBox::new(Orientation::Vertical, 8);
    activity_card.add_css_class("panel");
    let activity_title = Label::new(Some("VIC activity"));
    activity_title.add_css_class("section-title-small");
    activity_title.set_halign(Align::Start);
    let activity_list = GtkBox::new(Orientation::Vertical, 7);
    activity_card.append(&activity_title);
    activity_card.append(&activity_list);
    let worker_card = GtkBox::new(Orientation::Vertical, 8);
    worker_card.add_css_class("panel");
    let worker_title = Label::new(Some("Hermes subagents"));
    worker_title.add_css_class("section-title-small");
    worker_title.set_halign(Align::Start);
    let worker_list = GtkBox::new(Orientation::Vertical, 7);
    worker_card.append(&worker_title);
    worker_card.append(&worker_list);
    let task_card = GtkBox::new(Orientation::Vertical, 8);
    task_card.add_css_class("panel");
    let task_title = Label::new(Some("Active tasks"));
    task_title.add_css_class("section-title-small");
    task_title.set_halign(Align::Start);
    let task_list = GtkBox::new(Orientation::Vertical, 7);
    task_card.append(&task_title);
    task_card.append(&task_list);
    operations.append(&memory_review_card);
    operations.append(&activity_card);
    operations.append(&worker_card);
    operations.append(&task_card);
    content.append(&operations);
    root.append(&content);
    window.set_child(Some(&root));

    {
        let state = state.clone();
        let sender = sender.clone();
        scan_memories.connect_clicked(move |button| {
            button.set_sensitive(false);
            let gateway = state.borrow().gateway.clone();
            let sender = sender.clone();
            thread::spawn(move || {
                let result = start_memory_scan(&gateway).and_then(|_| load_memory_review(&gateway));
                let _ = sender.send(UiEvent::MemoryReview(result));
            });
        });
    }

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
            let attachment_ids = state
                .borrow()
                .pending_attachment
                .iter()
                .map(|item| item.id.clone())
                .collect();
            let tx = sender.clone();
            thread::spawn(move || {
                let result = send_turn(&gateway, session.as_deref(), &text, attachment_ids);
                let _ = tx.send(UiEvent::TurnFinished {
                    user_text: text,
                    result,
                });
            });
        }
    };
    let submit_button = submit.clone();
    send.connect_clicked(move |_| submit_button());
    entry.connect_activate(move |_| submit());

    {
        let state = state.clone();
        let sender = sender.clone();
        let status = status.clone();
        let spinner = spinner.clone();
        let orb_button = orb.clone();
        orb.connect_clicked(move |_| {
            let mut current = state.borrow_mut();
            if current.busy {
                return;
            }
            if let Some(mut recorder) = current.recorder.take() {
                let _ = Command::new("kill")
                    .args(["-INT", &recorder.id().to_string()])
                    .status();
                let _ = recorder.wait();
                let Some(path) = current.recording_path.take() else {
                    return;
                };
                current.busy = true;
                status.set_text("TRANSCRIBING");
                spinner.set_visible(true);
                spinner.start();
                orb_button.set_label("VIC\nREADY");
                let gateway = current.gateway.clone();
                let session = current.session_id.clone();
                let tx = sender.clone();
                thread::spawn(move || {
                    let result = transcribe_recording(&gateway, &path).and_then(|text| {
                        send_turn(&gateway, session.as_deref(), &text, Vec::new())
                            .map(|turn| (text, turn))
                    });
                    let _ = std::fs::remove_file(path);
                    match result {
                        Ok((text, turn)) => {
                            let _ = tx.send(UiEvent::TurnFinished {
                                user_text: text,
                                result: Ok(turn),
                            });
                        }
                        Err(error) => {
                            let _ = tx.send(UiEvent::TurnFinished {
                                user_text: String::new(),
                                result: Err(error),
                            });
                        }
                    }
                });
                return;
            }
            let path =
                env::temp_dir().join(format!("vic-native-recording-{}.wav", std::process::id()));
            match Command::new("pw-record")
                .args(["--rate", "16000", "--channels", "1", "--format", "s16"])
                .arg(&path)
                .spawn()
            {
                Ok(child) => {
                    current.recorder = Some(child);
                    current.recording_path = Some(path);
                    status.set_text("LISTENING");
                    orb_button.set_label("STOP\nLISTENING");
                }
                Err(error) => status.set_text(&format!("MIC ERROR · {error}")),
            }
        });
    }

    let event_state = state.clone();
    let event_status = status.clone();
    let event_spinner = spinner.clone();
    let event_entry = entry.clone();
    let event_sender = sender.clone();
    glib::timeout_add_local(Duration::from_millis(80), move || {
        while let Ok(event) = receiver.try_recv() {
            match event {
                UiEvent::Loaded(Ok(active)) => {
                    let mut current = event_state.borrow_mut();
                    current.session_id = Some(active.conversation_id);
                    current.messages = active.messages;
                    event_status.set_text("ONLINE");
                    render_messages(&message_list, &current.messages, &current.gateway);
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
                            let next = current
                                .messages
                                .last()
                                .map_or(1, |message| message.sequence + 1);
                            let attachments =
                                current.pending_attachment.take().into_iter().collect();
                            current.messages.push(Message {
                                sequence: next,
                                role: "user".into(),
                                content: user_text,
                                provider: None,
                                attachments,
                            });
                            let spoken_reply = turn.response_text.clone();
                            current.messages.push(Message {
                                sequence: next + 1,
                                role: "assistant".into(),
                                content: turn.response_text,
                                provider: Some(turn.provider),
                                attachments: Vec::new(),
                            });
                            current.session_id = Some(turn.session_id);
                            event_status.set_text("ONLINE");
                            attachment_status.set_text("");
                            render_messages(&message_list, &current.messages, &current.gateway);
                            let gateway = current.gateway.clone();
                            thread::spawn(move || speak_reply(&gateway, &spoken_reply));
                        }
                        Err(error) => event_status.set_text(&format!("ERROR · {error}")),
                    }
                }
                UiEvent::AttachmentUploaded(Ok(attachment)) => {
                    attachment_status.set_text(&format!("{} ready", attachment.filename));
                    event_state.borrow_mut().pending_attachment = Some(attachment);
                }
                UiEvent::AttachmentUploaded(Err(error)) => {
                    event_status.set_text(&format!("IMAGE ERROR · {error}"))
                }
                UiEvent::Dashboard(Ok(update)) => {
                    if !update.activities.is_empty() {
                        render_compact_list(
                            &activity_list,
                            &update.activities,
                            "Execution rail ready",
                        );
                    }
                    if !update.workers.is_empty() {
                        render_compact_list(&worker_list, &update.workers, "No active workers");
                    }
                    render_compact_list(&task_list, &update.tasks, "No active tasks");
                }
                UiEvent::Dashboard(Err(_)) => {}
                UiEvent::MemoryReview(Ok(reports)) => {
                    scan_memories.set_sensitive(true);
                    render_memory_review(
                        &memory_review_list,
                        &reports,
                        &event_state.borrow().gateway,
                        &event_sender,
                    );
                }
                UiEvent::MemoryReview(Err(error)) => {
                    scan_memories.set_sensitive(true);
                    render_compact_list(
                        &memory_review_list,
                        &[format!("Memory review unavailable · {error}")],
                        "No pending memory proposals",
                    );
                }
                UiEvent::MemoryAction(Ok(())) => {
                    let gateway = event_state.borrow().gateway.clone();
                    let sender = event_sender.clone();
                    thread::spawn(move || {
                        let _ = sender.send(UiEvent::MemoryReview(load_memory_review(&gateway)));
                    });
                }
                UiEvent::MemoryAction(Err(error)) => {
                    event_status.set_text(&format!("MEMORY ERROR · {error}"));
                }
            }
        }
        glib::ControlFlow::Continue
    });

    let gateway = state.borrow().gateway.clone();
    let initial_sender = sender.clone();
    thread::spawn(move || {
        let _ = initial_sender.send(UiEvent::Loaded(load_conversation(&gateway)));
    });
    let dashboard_gateway = state.borrow().gateway.clone();
    let dashboard_sender = sender.clone();
    thread::spawn(move || {
        let mut cursor = 0;
        loop {
            let update = load_dashboard(&dashboard_gateway, cursor);
            if let Ok(value) = &update {
                cursor = value.cursor;
            }
            if dashboard_sender.send(UiEvent::Dashboard(update)).is_err() {
                break;
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
    let memory_gateway = state.borrow().gateway.clone();
    let memory_sender = sender.clone();
    thread::spawn(move || {
        loop {
            if memory_sender
                .send(UiEvent::MemoryReview(load_memory_review(&memory_gateway)))
                .is_err()
            {
                break;
            }
            thread::sleep(Duration::from_secs(10));
        }
    });
    window.present();
}

fn render_memory_review(
    container: &GtkBox,
    reports: &[SleepCycleReport],
    gateway: &str,
    sender: &mpsc::Sender<UiEvent>,
) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    let pending = reports
        .iter()
        .filter(|report| report.cycle.mode == "dry_run")
        .flat_map(|report| {
            report
                .changes
                .iter()
                .filter(|change| change.status == "proposed")
                .map(move |change| (&report.cycle, change))
        })
        .take(5)
        .collect::<Vec<_>>();
    if pending.is_empty() {
        let label = Label::new(Some("No pending memory proposals"));
        label.add_css_class("muted-small");
        label.set_halign(Align::Start);
        container.append(&label);
        return;
    }
    for (cycle, change) in pending {
        let row = GtkBox::new(Orientation::Vertical, 6);
        row.add_css_class("memory-proposal");
        let detail = Label::new(Some(&change.detail));
        detail.set_wrap(true);
        detail.set_halign(Align::Start);
        detail.set_xalign(0.0);
        row.append(&detail);
        let controls = GtkBox::new(Orientation::Horizontal, 6);
        let confidence = Label::new(Some(&format!(
            "{} · {:.0}%",
            compact_date(&cycle.created_at),
            change.confidence.unwrap_or(0.0) * 100.0
        )));
        confidence.add_css_class("muted-small");
        confidence.set_hexpand(true);
        confidence.set_halign(Align::Start);
        let approve = Button::with_label("Remember");
        approve.add_css_class("approve-button");
        let gateway = gateway.to_owned();
        let cycle_id = cycle.id.clone();
        let change_id = change.id.clone();
        let sender = sender.clone();
        approve.connect_clicked(move |button| {
            button.set_sensitive(false);
            let gateway = gateway.clone();
            let cycle_id = cycle_id.clone();
            let change_id = change_id.clone();
            let sender = sender.clone();
            thread::spawn(move || {
                let result = commit_memory_proposals(&gateway, &cycle_id, &[change_id]);
                let _ = sender.send(UiEvent::MemoryAction(result));
            });
        });
        controls.append(&confidence);
        controls.append(&approve);
        row.append(&controls);
        container.append(&row);
    }
}

fn compact_date(value: &str) -> &str {
    value.get(0..10).unwrap_or(value)
}

fn render_compact_list(container: &GtkBox, items: &[String], empty: &str) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    let visible: Vec<&String> = items.iter().take(5).collect();
    if visible.is_empty() {
        let label = Label::new(Some(empty));
        label.add_css_class("muted-small");
        label.set_halign(Align::Start);
        container.append(&label);
        return;
    }
    for item in visible {
        let label = Label::new(Some(item));
        label.add_css_class("operation-row");
        label.set_wrap(true);
        label.set_halign(Align::Start);
        label.set_xalign(0.0);
        container.append(&label);
    }
}

fn render_messages(container: &GtkBox, messages: &[Message], gateway: &str) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    let latest_assistant = messages
        .iter()
        .rposition(|message| message.role == "assistant");
    for (index, message) in messages.iter().enumerate() {
        if message.role != "user" && message.role != "assistant" {
            continue;
        }
        let card = GtkBox::new(Orientation::Vertical, 5);
        card.add_css_class("message");
        card.add_css_class(if message.role == "user" {
            "message-user"
        } else {
            "message-vic"
        });
        let role = if message.role == "user" { "YOU" } else { "VIC" };
        let meta = message.provider.as_deref().unwrap_or("");
        let heading = Label::new(Some(&format!("{role}  {meta}")));
        heading.add_css_class("message-role");
        heading.set_halign(Align::Start);
        card.append(&heading);
        for attachment in &message.attachments {
            if let Ok(path) = cache_attachment(gateway, attachment) {
                let picture = Picture::for_filename(path);
                picture.set_can_shrink(true);
                picture.set_size_request(-1, 280);
                picture.set_tooltip_text(Some(&attachment.filename));
                card.append(&picture);
            }
        }
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
    response
        .body_mut()
        .read_json()
        .map_err(|error| error.to_string())
}

fn load_memory_review(gateway: &str) -> Result<Vec<SleepCycleReport>, String> {
    let mut response = ureq::get(format!("{gateway}/v1/memory/sleep-cycles?limit=14"))
        .header("X-VoiceOS-Device-ID", DEVICE_ID)
        .call()
        .map_err(|error| error.to_string())?;
    let payload: SleepCycleListResponse = response
        .body_mut()
        .read_json()
        .map_err(|error| error.to_string())?;
    Ok(payload.sleep_cycles)
}

fn start_memory_scan(gateway: &str) -> Result<(), String> {
    let key = format!(
        "vic-panel-scan-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );
    ureq::post(format!("{gateway}/v1/memory/sleep-cycles"))
        .header("X-VoiceOS-Device-ID", DEVICE_ID)
        .send_json(serde_json::json!({"idempotency_key": key}))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn commit_memory_proposals(
    gateway: &str,
    sleep_cycle_id: &str,
    change_ids: &[String],
) -> Result<(), String> {
    let key = format!("vic-panel-commit-{sleep_cycle_id}-{}", change_ids.join("-"));
    ureq::post(format!(
        "{gateway}/v1/memory/sleep-cycles/{sleep_cycle_id}/commit"
    ))
    .header("X-VoiceOS-Device-ID", DEVICE_ID)
    .send_json(serde_json::json!({
        "idempotency_key": key,
        "change_ids": change_ids,
    }))
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn send_turn(
    gateway: &str,
    session_id: Option<&str>,
    text: &str,
    attachment_ids: Vec<String>,
) -> Result<TurnResponse, String> {
    let request = TurnRequest {
        session_id,
        text,
        request_id: format!(
            "vic-native-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ),
        attachment_ids,
    };
    let mut response = ureq::post(format!("{gateway}/v1/turns/text"))
        .header("X-VoiceOS-Device-ID", DEVICE_ID)
        .send_json(&request)
        .map_err(|error| error.to_string())?;
    response
        .body_mut()
        .read_json()
        .map_err(|error| error.to_string())
}

fn cache_attachment(gateway: &str, attachment: &Attachment) -> Result<PathBuf, String> {
    let extension = match attachment.media_type.as_str() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        _ => return Err("Unsupported image type".into()),
    };
    let directory = env::temp_dir().join("vic-native-attachments");
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join(format!("{}.{}", attachment.id, extension));
    if !path.is_file() {
        let mut response = ureq::get(format!("{gateway}/v1/attachments/{}", attachment.id))
            .header("X-VoiceOS-Device-ID", DEVICE_ID)
            .call()
            .map_err(|error| error.to_string())?;
        let bytes = response
            .body_mut()
            .read_to_vec()
            .map_err(|error| error.to_string())?;
        std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
    }
    Ok(path)
}

fn transcribe_recording(gateway: &str, path: &PathBuf) -> Result<String, String> {
    let audio = std::fs::read(path).map_err(|error| error.to_string())?;
    let mut response = ureq::post(format!("{gateway}/v1/transcriptions"))
        .header("X-VoiceOS-Device-ID", DEVICE_ID)
        .content_type("audio/wav")
        .send(&audio)
        .map_err(|error| error.to_string())?;
    let transcription: TranscriptionResponse = response
        .body_mut()
        .read_json()
        .map_err(|error| error.to_string())?;
    if transcription.transcript.trim().is_empty() {
        return Err("No speech was detected".into());
    }
    Ok(transcription.transcript.trim().to_owned())
}

fn format_task_card(detail: &Value) -> Option<String> {
    let task = detail.get("task")?;
    let title = task.get("title")?.as_str()?;
    let status = task
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("active");
    let progress = detail.get("progress");
    let completed = progress
        .and_then(|value| value.get("completed_steps"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let total = progress
        .and_then(|value| value.get("total_steps"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let blockers = progress
        .and_then(|value| value.get("open_blockers"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let lane = progress
        .and_then(|value| value.get("lane"))
        .and_then(Value::as_str)
        .unwrap_or("triage");
    let next = progress
        .and_then(|value| value.get("next_vic_action"))
        .and_then(Value::as_str)
        .or_else(|| {
            progress
                .and_then(|value| value.get("next_user_action"))
                .and_then(Value::as_str)
        });
    let next = next
        .map(|value| format!(" · next: {}", compact(value, 72)))
        .unwrap_or_default();
    let blocker_note = if blockers > 0 {
        format!(" · blockers: {blockers}")
    } else {
        String::new()
    };
    let stage = if completed == 0 {
        "scope"
    } else if total > 0 && completed < total {
        "work"
    } else {
        "verify"
    };
    Some(format!(
        "{status} · {completed}/{total} · {lane} · stage: {stage}{blocker_note} · {title}{next}"
    ))
}

fn compact(value: &str, max: usize) -> String {
    let value = value.trim();
    if value.chars().count() <= max {
        return value.to_owned();
    }
    let shortened: String = value.chars().take(max.saturating_sub(1)).collect();
    format!("{shortened}…")
}

fn load_dashboard(gateway: &str, cursor: i64) -> Result<DashboardUpdate, String> {
    let tail = if cursor == 0 { "&tail=true" } else { "" };
    let mut event_response =
        ureq::get(format!("{gateway}/v1/events/recovery?after={cursor}{tail}"))
            .header("X-VoiceOS-Device-ID", DEVICE_ID)
            .call()
            .map_err(|error| error.to_string())?;
    let recovery: RecoveryResponse = event_response
        .body_mut()
        .read_json()
        .map_err(|error| error.to_string())?;
    let mut activities = Vec::new();
    let mut workers = Vec::new();
    for event in recovery.events.iter().rev() {
        let label = event
            .payload
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("VIC update");
        let detail = event
            .payload
            .get("detail")
            .and_then(Value::as_str)
            .unwrap_or("");
        let line = if detail.is_empty() {
            label.to_owned()
        } else {
            format!("{label} · {detail}")
        };
        match event.event_type.as_str() {
            "agent.activity.updated" if activities.len() < 5 => activities.push(line),
            "agent.worker.updated" if workers.len() < 5 => {
                let status = event
                    .payload
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("working");
                workers.push(format!("{status} · {line}"));
            }
            _ => {}
        }
    }
    let mut task_response = ureq::get(format!("{gateway}/v1/tasks?limit=12"))
        .header("X-VoiceOS-Device-ID", DEVICE_ID)
        .call()
        .map_err(|error| error.to_string())?;
    let task_payload: Value = task_response
        .body_mut()
        .read_json()
        .map_err(|error| error.to_string())?;
    let tasks = task_payload
        .get("details")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(format_task_card)
        .take(5)
        .collect();
    Ok(DashboardUpdate {
        cursor: recovery.latest_event_id,
        activities,
        workers,
        tasks,
    })
}

fn speak_reply(gateway: &str, text: &str) {
    let Ok(mut response) = ureq::post(format!("{gateway}/v1/speech/synthesize"))
        .header("X-VoiceOS-Device-ID", DEVICE_ID)
        .send_json(serde_json::json!({"text": text}))
    else {
        return;
    };
    let Ok(audio) = response.body_mut().read_to_vec() else {
        return;
    };
    let path = env::temp_dir().join(format!("vic-native-{}.mp3", std::process::id()));
    if std::fs::write(&path, audio).is_err() {
        return;
    }
    let _ = Command::new("pw-play").arg(&path).status();
    let _ = std::fs::remove_file(path);
}

fn install_styles() {
    let provider = CssProvider::new();
    provider.load_from_data(include_str!("style.css"));
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
        assert_eq!(
            first_sentence("First result. More detail."),
            "First result."
        );
    }
}
