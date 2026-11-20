mod provider;
mod theme;

use provider::{Message as ProviderMessage, MockProvider, OpenAICompatProvider, Provider};
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, ScrollArea, SidePanel, TextEdit, Ui};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("openchat"),
        ..Default::default()
    };
    eframe::run_native(
        "openchat",
        options,
        Box::new(|creation_context| {
            theme::apply(&creation_context.egui_ctx);
            Ok(Box::new(OpenChatApp::default()))
        }),
    )
}

/// A chat session shown in the left conversation list.
struct Conversation {
    id: u64,
    title: String,
    preview: String,
}

/// A message rendered in the center chat pane.
#[derive(Clone)]
struct Message {
    role: Role,
    body: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    User,
    Assistant,
    System,
}

/// Placeholder skill entry shown in the right skills panel until
/// the skills integration (Phase 3+) lands.
struct Skill {
    name: String,
    description: String,
}

#[derive(Default)]
struct OpenChatApp {
    conversations: Vec<Conversation>,
    messages: Vec<Message>,
    skills: Vec<Skill>,
    selected: Option<u64>,
    draft: String,
    provider: ProviderChoice,
    reply_rx: Option<Receiver<Result<String, String>>>,
    pending: bool,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum ProviderChoice {
    #[default]
    Mock,
    OpenAICompat,
}

impl OpenChatApp {
    fn select_conversation(&mut self, id: u64) {
        self.selected = Some(id);
        // Sessions are still local to this app shell; each selection starts a clean transcript.
        self.messages.clear();
    }
}

impl eframe::App for OpenChatApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(receiver) = &self.reply_rx {
            if let Ok(result) = receiver.try_recv() {
                self.pending = false;
                self.reply_rx = None;
                self.messages.push(Message {
                    role: Role::Assistant,
                    body: result.unwrap_or_else(|error| format!("Provider error: {error}")),
                });
            }
        }
        if self.conversations.is_empty() {
            // Placeholder conversations until sessions come from the daemon.
            self.conversations = vec![
                Conversation {
                    id: 1,
                    title: "Welcome".to_string(),
                    preview: "Placeholder session".to_string(),
                },
                Conversation {
                    id: 2,
                    title: "Scratch".to_string(),
                    preview: "Second placeholder".to_string(),
                },
            ];
            self.skills = vec![Skill {
                name: "example-skill".to_string(),
                description: "Placeholder until skill discovery is wired into the UI.".to_string(),
            }];
        }

        self.conversations_panel(context);
        self.skills_panel(context);
        self.chat_panel(context);
        if self.pending { context.request_repaint_after(std::time::Duration::from_millis(50)); }
    }
}

impl OpenChatApp {
    fn conversations_panel(&mut self, context: &egui::Context) {
        SidePanel::left("conversations")
            .resizable(true)
            .default_width(220.0)
            .show(context, |ui| {
                ui.heading("Chats");
                ui.separator();
                ScrollArea::vertical().show(ui, |ui| {
                    let selected = self.selected;
                    for conversation in &self.conversations {
                        let is_selected = selected == Some(conversation.id);
                        if ui
                            .selectable_label(is_selected, &conversation.title)
                            .clicked()
                        {
                            self.select_conversation(conversation.id);
                        }
                        ui.small(&conversation.preview);
                    }
                });
            });
    }

    fn skills_panel(&mut self, context: &egui::Context) {
        SidePanel::right("skills")
            .resizable(true)
            .default_width(260.0)
            .show(context, |ui| {
                ui.heading("Provider");
                ui.radio_value(&mut self.provider, ProviderChoice::Mock, "Mock");
                let configured = OpenAICompatProvider::from_env().is_some();
                ui.add_enabled_ui(configured, |ui| {
                    ui.radio_value(
                        &mut self.provider,
                        ProviderChoice::OpenAICompat,
                        "OpenAI-compatible",
                    );
                });
                if !configured {
                    ui.small("Configure OPENCHAT_BASE_URL and OPENCHAT_API_KEY to enable.");
                }
                ui.separator();
                ui.heading("Skills");
                ui.separator();
                ScrollArea::vertical().show(ui, |ui| {
                    for skill in &self.skills {
                        ui.group(|ui| {
                            ui.strong(&skill.name);
                            ui.small(&skill.description);
                        });
                    }
                });
            });
    }

    fn chat_panel(&mut self, context: &egui::Context) {
        egui::CentralPanel::default().show(context, |ui| {
            if self.selected.is_none() {
                centered_text(ui, "Select a conversation to begin.");
                return;
            }
            ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    for message in &self.messages {
                        message_row(ui, message);
                    }
                });
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.separator();
                ui.horizontal(|ui| {
                    let response = ui.add(
                        TextEdit::singleline(&mut self.draft)
                            .hint_text("Type a message…")
                            .desired_width(ui.available_width() - 80.0),
                    );
                    let send = ui.button("Send");
                    let submitted = send.clicked()
                        || (response.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter)));
                    if submitted && !self.draft.trim().is_empty() && !self.pending {
                        let body = self.draft.trim().to_string();
                        self.messages.push(Message { role: Role::User, body: body.clone() });
                        if let Some(conversation) = self.conversations.iter_mut()
                            .find(|conversation| Some(conversation.id) == self.selected)
                        {
                            conversation.preview = body.clone();
                        }
                        self.draft.clear();
                        let messages = self.messages.iter().map(|message| ProviderMessage {
                            role: match message.role {
                                Role::User => "user",
                                Role::Assistant => "assistant",
                                Role::System => "system",
                            }.to_string(),
                            content: message.body.clone(),
                        }).collect::<Vec<_>>();
                        let provider: Box<dyn Provider> = match self.provider {
                            ProviderChoice::Mock => Box::new(MockProvider),
                            ProviderChoice::OpenAICompat => match OpenAICompatProvider::from_env() {
                                Some(provider) => Box::new(provider),
                                None => {
                                    self.messages.push(Message { role: Role::Assistant,
                                        body: "OpenAI-compatible provider is not configured.".into() });
                                    return;
                                }
                            },
                        };
                        let (sender, receiver) = mpsc::channel();
                        self.reply_rx = Some(receiver);
                        self.pending = true;
                        std::thread::spawn(move || {
                            let _ = sender.send(provider.send(&messages));
                        });
                    }
                    if self.pending {
                        ui.spinner();
                        ui.small("Waiting for provider…");
                    }
                    if send.clicked() {
                        response.request_focus();
                    }
                });
            });
        });
    }
}

fn message_row(ui: &mut Ui, message: &Message) {
    match message.role {
        Role::System => {
            ui.vertical_centered(|ui| {
                ui.small(message.body.clone());
            });
        }
        Role::User => {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.add_sized(
                    [ui.available_width() * 0.6, 0.0],
                    egui::Label::new(egui::RichText::new(message.body.clone()).background_color(
                        theme::ACCENT.gamma_multiply(0.2),
                    )),
                );
            });
        }
        Role::Assistant => {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.add_sized(
                    [ui.available_width() * 0.6, 0.0],
                    egui::Label::new(egui::RichText::new(message.body.clone()).background_color(
                        theme::PANEL,
                    )),
                );
            });
        }
    }
    ui.add_space(4.0);
}

fn centered_text(ui: &mut Ui, text: &str) {
    ui.with_layout(egui::Layout::top_down_justified(egui::Align::Center), |ui| {
        ui.add_space(ui.available_height() * 0.4);
        ui.weak(text);
    });
}
