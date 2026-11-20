#![allow(dead_code, clippy::possible_missing_else)]
mod persist;
mod provider;
mod subagents;
mod theme;

use eframe::egui::{self, ScrollArea, SidePanel, TextEdit, Ui};
use openchat_slop::{prompt::slop_law_prompt, SlopLint, SlopLintConfig};
use provider::{
    Message as ProviderMessage, MockProvider, OpenAICompatProvider, OpenCodeProvider, Provider,
};
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

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
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(OpenChatApp::load_or_default()))
        }),
    )
}
struct Conversation {
    id: u64,
    title: String,
    preview: String,
    provider: ProviderChoice,
    model_override: String,
    base_url_override: String,
}
#[derive(Clone, PartialEq, Debug)]
struct Message {
    role: Role,
    body: String,
}
#[derive(Clone, Copy, PartialEq, Debug)]
enum Role {
    User,
    Assistant,
    System,
}
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
    reply_rx: Option<Receiver<StreamEvent>>,
    pending: bool,
    subagents: Vec<subagents::SubagentStatus>,
    selected_messages: Vec<usize>,
    selection_anchor: Option<usize>,
    attachment_entry: String,
    saved_image: Option<String>,
    slop_law: bool,
    lint_output: Option<String>,
}
#[derive(Clone, Copy, Default, PartialEq, Debug)]
enum ProviderChoice {
    #[default]
    Mock,
    OpenAICompat,
    OpenCode,
}
enum StreamEvent {
    Chunk(String),
    Finished(Result<(), String>),
}
#[derive(Clone, Copy)]
enum SelectionAction {
    Select(usize),
    ToHere(usize),
    Toggle(usize, bool),
}
impl OpenChatApp {
    fn load_or_default() -> Self {
        let mut app = Self::default();
        if let Some((conversations, messages, selected)) = persist::load(persist::DEFAULT_PATH) {
            app.conversations = conversations;
            app.messages = messages;
            app.selected = selected.filter(|id| app.conversations.iter().any(|c| c.id == *id));
        }
        app
    }
    fn save_state(&self) {
        persist::save(
            persist::DEFAULT_PATH,
            &self.conversations,
            &self.messages,
            self.selected,
        );
    }
    fn select_conversation(&mut self, id: u64) {
        self.selected = Some(id);
        self.messages.clear();
        self.selected_messages.clear();
        self.selection_anchor = None;
        self.lint_output = None;
        self.save_state();
    }
    fn insert_attachment(&mut self, kind: &str) {
        let path = self.attachment_entry.trim();
        if path.is_empty() {
            return;
        }
        if !self.draft.is_empty() && !self.draft.ends_with(' ') {
            self.draft.push(' ');
        }
        self.draft.push_str(&format!("[[{kind}:{path}]]"));
        self.attachment_entry.clear();
    }
    fn run_slop_lint(&mut self) {
        let lint = SlopLint::new(SlopLintConfig::default());
        let mut report = String::new();
        for (index, message) in self.messages.iter().enumerate() {
            let findings = lint.check(&message.body);
            if !findings.is_empty() {
                report.push_str(&format!(
                    "#{} ({:?}): {:?}\n",
                    index + 1,
                    message.role,
                    findings
                ));
            }
        }
        self.lint_output = Some(if report.is_empty() {
            "No slop found.".into()
        } else {
            report
        });
    }
}
impl eframe::App for OpenChatApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut events = Vec::new();
        let mut disconnected = false;
        if let Some(rx) = &self.reply_rx {
            loop {
                match rx.try_recv() {
                    Ok(event) => {
                        let done = matches!(&event, StreamEvent::Finished(_));
                        events.push(event);
                        if done {
                            break;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        let mut finished = false;
        for event in events {
            match event {
                StreamEvent::Chunk(chunk) => {
                    if let Some(m) = self
                        .messages
                        .iter_mut()
                        .rev()
                        .find(|m| m.role == Role::Assistant)
                    {
                        m.body.push_str(&chunk)
                    }
                }
                StreamEvent::Finished(result) => {
                    if let Err(error) = result {
                        self.messages.push(Message {
                            role: Role::Assistant,
                            body: format!("Provider error: {error}"),
                        });
                    }
                    finished = true;
                }
            }
        }
        if finished || disconnected {
            if disconnected && !finished {
                self.messages.push(Message {
                    role: Role::Assistant,
                    body: "Provider error: response stream ended unexpectedly.".into(),
                });
            }
            self.pending = false;
            self.reply_rx = None;
            self.save_state();
        }
        if self.conversations.is_empty() {
            self.conversations = vec![
                Conversation {
                    id: 1,
                    title: "Welcome".into(),
                    preview: "Placeholder session".into(),
                    provider: ProviderChoice::Mock,
                    model_override: String::new(),
                    base_url_override: String::new(),
                },
                Conversation {
                    id: 2,
                    title: "Scratch".into(),
                    preview: "Second placeholder".into(),
                    provider: ProviderChoice::Mock,
                    model_override: String::new(),
                    base_url_override: String::new(),
                },
            ];
            self.skills = vec![Skill {
                name: "example-skill".into(),
                description: "Placeholder until skill discovery is wired into the UI.".into(),
            }];
        }
        if self.subagents.is_empty() {
            self.subagents = subagents::demo_agents();
        }
        let now = Instant::now();
        for agent in &mut self.subagents {
            agent.advance_demo(now);
        }
        self.conversations_panel(ctx);
        self.skills_panel(ctx);
        self.chat_panel(ctx);
        if self.pending {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}
impl OpenChatApp {
    fn conversations_panel(&mut self, ctx: &egui::Context) {
        SidePanel::left("conversations")
            .resizable(true)
            .default_width(220.0)
            .show(ctx, |ui| {
                ui.heading("Chats");
                ui.separator();
                ui.add_enabled_ui(!self.pending, |ui| {
                    ScrollArea::vertical().show(ui, |ui| {
                        let selected = self.selected;
                        let ids: Vec<u64> = self.conversations.iter().map(|c| c.id).collect();
                        for id in ids {
                            let title = self
                                .conversations
                                .iter()
                                .find(|c| c.id == id)
                                .map(|c| c.title.clone())
                                .unwrap_or_default();
                            let preview = self
                                .conversations
                                .iter()
                                .find(|c| c.id == id)
                                .map(|c| c.preview.clone())
                                .unwrap_or_default();
                            if ui.selectable_label(selected == Some(id), &title).clicked() {
                                self.select_conversation(id);
                            }
                            ui.small(&preview);
                        }
                    });
                });
            });
    }
    fn skills_panel(&mut self, ctx: &egui::Context) {
        SidePanel::right("skills")
            .resizable(true)
            .default_width(260.0)
            .show(ctx, |ui| {
                ui.heading("Conversation provider");
                if let Some(c) = self
                    .conversations
                    .iter_mut()
                    .find(|c| Some(c.id) == self.selected)
                {
                    ui.radio_value(&mut c.provider, ProviderChoice::Mock, "Mock");
                    let configured = std::env::var("OPENCHAT_API_KEY")
                        .map(|k| !k.trim().is_empty())
                        .unwrap_or(false)
                        && (std::env::var("OPENCHAT_BASE_URL")
                            .map(|u| !u.trim().is_empty())
                            .unwrap_or(false)
                            || !c.base_url_override.trim().is_empty());
                    ui.add_enabled_ui(configured, |ui| {
                        ui.radio_value(
                            &mut c.provider,
                            ProviderChoice::OpenAICompat,
                            "OpenAI-compatible",
                        );
                    });
                    if !configured {
                        ui.small(
                            "Set OPENCHAT_API_KEY and OPENCHAT_BASE_URL, or enter a URL override.",
                        );
                    }
                    let opencode_configured = std::env::var("OPENCODE_URL")
                        .map(|u| !u.trim().is_empty())
                        .unwrap_or(false);
                    ui.add_enabled_ui(opencode_configured, |ui| {
                        ui.radio_value(&mut c.provider, ProviderChoice::OpenCode, "OpenCode");
                    });
                    if !opencode_configured {
                        ui.small(
                            "Set OPENCODE_URL (and optional OPENCODE_PASSWORD) to use OpenCode.",
                        );
                    }
                    ui.label("Model override (optional)");
                    ui.add(
                        TextEdit::singleline(&mut c.model_override)
                            .hint_text("Use OPENCHAT_MODEL default"),
                    );
                    ui.label("Base URL override (optional)");
                    ui.add(
                        TextEdit::singleline(&mut c.base_url_override)
                            .hint_text("Use OPENCHAT_BASE_URL default"),
                    );
                } else {
                    ui.small("Select a conversation to configure its provider.");
                }
                ui.separator();
                ui.checkbox(&mut self.slop_law, "Slop Law prompt");
                ui.small("Prepends the Slop Law rules to the system prompt on send.");
                ui.separator();
                ui.heading("Subagents");
                ui.small("Demo only — no real subagent orchestration is running.");
                for a in &self.subagents {
                    ui.group(|ui| {
                        ui.strong(a.name);
                        ui.small(a.current_step());
                    });
                }
                ui.heading("Work status");
                for a in &self.subagents {
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::LIGHT_GREEN, "●");
                        ui.label(format!("{} — {}", a.name, a.current_step()));
                    });
                }
                ui.separator();
                ui.heading("Skills");
                for s in &self.skills {
                    ui.group(|ui| {
                        ui.strong(&s.name);
                        ui.small(&s.description);
                    });
                }
            });
    }
    fn chat_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx,|ui|{
        if self.selected.is_none(){ui.vertical_centered(|ui|{ui.add_space(ui.available_height()*0.4);ui.weak("Select a conversation to begin.");});return;}
        if !self.selected_messages.is_empty(){ui.horizontal(|ui|{ui.strong(format!("{} selected",self.selected_messages.len()));if ui.button("Copy as Markdown").clicked(){let md=self.selected_messages.iter().filter_map(|&i|self.messages.get(i)).map(|m|format!("**{}**\n\n{}",role_label(m.role),m.body)).collect::<Vec<_>>().join("\n\n---\n\n");ui.output_mut(|o|o.copied_text=md);}
            if ui.button("Save as image (SVG)").clicked(){let text=self.selected_messages.iter().filter_map(|&i|self.messages.get(i)).map(|m|format!("{}: {}",role_label(m.role),m.body)).collect::<Vec<_>>().join("\n\n");match std::fs::write("openchat-selection.svg",transcript_svg(&text)){Ok(())=>self.saved_image=Some("Saved openchat-selection.svg".into()),Err(e)=>self.saved_image=Some(format!("Could not save image: {e}"))}}if ui.button("Clear").clicked(){self.selected_messages.clear();self.selection_anchor=None;}});if let Some(status)=&self.saved_image{ui.small(status);}}
        let mut actions=Vec::new();ScrollArea::vertical().auto_shrink(false).show(ui,|ui|{let mode=!self.selected_messages.is_empty();for(i,m)in self.messages.iter().enumerate(){if let Some(a)=message_row(ui,m,i,self.selected_messages.contains(&i),mode){actions.push(a);}}});for a in actions{match a{SelectionAction::Select(i)=>{self.selected_messages=vec![i];self.selection_anchor=Some(i)},SelectionAction::ToHere(i)=>{let a=self.selection_anchor.unwrap_or(i);self.selected_messages=(a.min(i)..=a.max(i)).collect()},SelectionAction::Toggle(i,shift)=>if shift{let a=self.selection_anchor.unwrap_or(i);self.selected_messages=(a.min(i)..=a.max(i)).collect()}else if self.selected_messages.contains(&i){self.selected_messages.retain(|&x|x!=i)}else{self.selected_messages.push(i);self.selection_anchor=Some(i)}}}
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center),|ui|{ui.separator();
            ui.horizontal(|ui|{if ui.button("Lint slop").clicked(){self.run_slop_lint();}if let Some(report)=&self.lint_output{ui.small(report);}});
            ui.horizontal(|ui|{let response=ui.add(TextEdit::multiline(&mut self.draft).hint_text("Type a message… pasted and quoted lines remain editable").desired_rows(3).desired_width(ui.available_width()-80.0));let send=ui.button("Send");let submitted=send.clicked()||(response.lost_focus()&&ui.input(|i|i.key_pressed(egui::Key::Enter)));if submitted&&!self.draft.trim().is_empty()&&!self.pending{
            let body=display_draft(&self.draft);self.messages.push(Message{role:Role::User,body:body.clone()});if let Some(c)=self.conversations.iter_mut().find(|c|Some(c.id)==self.selected){c.preview=body.clone();}self.draft.clear();
            let mut messages=self.messages.iter().map(|m|ProviderMessage{role:match m.role{Role::User=>"user",Role::Assistant=>"assistant",Role::System=>"system"}.into(),content:m.body.clone()}).collect::<Vec<_>>();
            if self.slop_law{messages.insert(0,ProviderMessage{role:"system".into(),content:slop_law_prompt()});}
            let(choice,model,url)=match self.conversations.iter().find(|c|Some(c.id)==self.selected){Some(c)=>(c.provider,c.model_override.clone(),c.base_url_override.clone()),None=>return};let provider:Result<Box<dyn Provider>,String>=match choice{ProviderChoice::Mock=>Ok(Box::new(MockProvider)),ProviderChoice::OpenAICompat=>OpenAICompatProvider::from_env_with_overrides(nonempty(&url),nonempty(&model)).map(|p|Box::new(p)as Box<dyn Provider>).ok_or_else(||"OpenAI-compatible provider is not configured (set OPENCHAT_API_KEY and a base URL).".into()),ProviderChoice::OpenCode=>OpenCodeProvider::from_env().map(|p|Box::new(p)as Box<dyn Provider>).ok_or_else(||"OpenCode provider is not configured (set OPENCODE_URL).".into())};let provider=match provider{Ok(p)=>p,Err(e)=>{self.messages.push(Message{role:Role::Assistant,body:format!("Provider error: {e}")});return}};self.messages.push(Message{role:Role::Assistant,body:String::new()});let(tx,rx)=mpsc::channel();self.reply_rx=Some(rx);self.pending=true;self.save_state();std::thread::spawn(move||{let result=provider.stream(&messages,&mut|chunk|{let _=tx.send(StreamEvent::Chunk(chunk.to_string()));});let _=tx.send(StreamEvent::Finished(result));});}
            if self.pending{ui.spinner();ui.small("Receiving reply…");}if send.clicked(){response.request_focus();}});
            ui.horizontal(|ui|{ui.add(TextEdit::singleline(&mut self.attachment_entry).hint_text("File path to attach"));if ui.button("Add file").clicked(){self.insert_attachment("file");}if ui.button("Add image").clicked(){self.insert_attachment("image");}});
            let mut remove=None;ui.horizontal_wrapped(|ui|{for(kind,segment)in composer_segments(&self.draft){if let Some(kind)=kind{let label=segment.rsplit('/').next().unwrap_or(&segment);ui.group(|ui|{ui.horizontal(|ui|{ui.label(format!("{} {label}",if kind=="image"{"🖼"}else{"📎"}));if ui.small_button("×").clicked(){remove=Some(format!("[[{kind}:{segment}]]"));} })});}else{ui.label(segment);}}});if let Some(token)=remove{self.draft=self.draft.replacen(&token,"",1);}
        });
    });
    }
}
fn nonempty(s: &str) -> Option<&str> {
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
fn role_label(r: Role) -> &'static str {
    match r {
        Role::User => "You",
        Role::Assistant => "Assistant",
        Role::System => "System",
    }
}
fn message_row(
    ui: &mut Ui,
    m: &Message,
    index: usize,
    selected: bool,
    selection_mode: bool,
) -> Option<SelectionAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        if selection_mode {
            let mut checked = selected;
            if ui.checkbox(&mut checked, "").clicked() {
                action = Some(SelectionAction::Toggle(
                    index,
                    ui.input(|i| i.modifiers.shift),
                ));
            }
        }
        let response = match m.role {
            Role::System => ui.small(m.body.clone()),
            Role::User => ui.add_sized(
                [ui.available_width() * 0.75, 0.0],
                egui::Label::new(
                    egui::RichText::new(m.body.clone())
                        .background_color(theme::ACCENT.gamma_multiply(0.2)),
                )
                .sense(egui::Sense::click()),
            ),
            Role::Assistant => ui.add_sized(
                [ui.available_width() * 0.75, 0.0],
                egui::Label::new(
                    egui::RichText::new(m.body.clone()).background_color(theme::PANEL),
                )
                .sense(egui::Sense::click()),
            ),
        };
        response.context_menu(|ui| {
            if ui.button("Select").clicked() {
                action = Some(SelectionAction::Select(index));
                ui.close_menu();
            }
            if ui.button("Select to here").clicked() {
                action = Some(SelectionAction::ToHere(index));
                ui.close_menu();
            }
        });
    });
    ui.add_space(4.0);
    action
}
fn composer_segments(text: &str) -> Vec<(Option<&'static str>, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        let f = rest.find("[[file:").map(|i| (i, "file", 7));
        let im = rest.find("[[image:").map(|i| (i, "image", 8));
        let next = match (f, im) {
            (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        let Some((start, kind, len)) = next else {
            break;
        };
        if start > 0 {
            out.push((None, rest[..start].into()));
        }
        let after = &rest[start + len..];
        if let Some(end) = after.find("]]") {
            out.push((Some(kind), after[..end].into()));
            rest = &after[end + 2..];
        } else {
            out.push((None, rest[start..].into()));
            return out;
        }
    }
    if !rest.is_empty() {
        out.push((None, rest.into()));
    }
    out
}
fn display_draft(text: &str) -> String {
    let mut result = text.to_string();
    for (kind, path) in composer_segments(text)
        .into_iter()
        .filter_map(|(k, v)| k.map(|k| (k, v)))
    {
        let label = path.rsplit('/').next().unwrap_or(&path);
        result = result.replace(
            &format!("[[{kind}:{path}]]"),
            &format!("[Attachment: {label}]"),
        );
    }
    result.trim().into()
}
fn transcript_svg(text: &str) -> String {
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    let lines = escaped.split('\n').collect::<Vec<_>>();
    let height = (lines.len().max(1) * 24 + 40).min(12000);
    let nodes=lines.iter().enumerate().map(|(i,line)|format!("<text x=\"24\" y=\"{}\" font-family=\"sans-serif\" font-size=\"16\" fill=\"#20242b\">{line}</text>",32+i*24)).collect::<String>();
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"900\" height=\"{height}\"><rect width=\"100%\" height=\"100%\" fill=\"#fff\"/>{nodes}</svg>")
}
