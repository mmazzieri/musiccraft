//! Dialogs. Each takes the focus, closes with Esc, and leaves the rest of the window alone.

use crate::app::{Action, App, set_theme};
use crate::i18n::{LANGUAGES, Lang, time};
use crate::theme::tokens;
use egui::{Key, Ui};
use musiccraft_engine::command::TagEdit;
use musiccraft_engine::{Command, Theme};
use musiccraft_library::Item;
use musiccraft_library::query::Sort;
use musiccraft_library::smart::{FIELDS, Field, Op, Rule, Rules};

/// A text of the tag editor: its first value and the one typed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Text {
    first: String,
    now: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    NewPlaylist { name: String, items: Vec<Item> },
    Smart { id: Option<String>, name: String, rules: Rules },
    Rename { id: String, name: String },
    DeletePlaylist { id: String, name: String },
    Remove { ids: Vec<String> },
    Info { ids: Vec<String>, fields: Vec<(Tag, Text)>, about: String },
    Station { name: String, url: String },
    Server { name: String, url: String, user: String, password: String },
    Soloist { path: String, key: String },
    Settings,
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Title,
    Artist,
    AlbumArtist,
    Album,
    Genre,
    Composer,
    Year,
    Track,
    Disc,
}

const TAGS: [Tag; 9] = [
    Tag::Title,
    Tag::Artist,
    Tag::AlbumArtist,
    Tag::Album,
    Tag::Genre,
    Tag::Composer,
    Tag::Year,
    Tag::Track,
    Tag::Disc,
];

impl Dialog {
    pub fn new_playlist(items: Vec<Item>) -> Dialog {
        Dialog::NewPlaylist { name: String::new(), items }
    }

    pub fn smart_new() -> Dialog {
        let rule = Rule { field: Field::Genre, op: Op::Contains, value: String::new() };
        Dialog::Smart { id: None, name: String::new(), rules: Rules { rules: vec![rule], ..Rules::default() } }
    }

    pub fn smart_edit(id: &str, name: &str, rules: Rules) -> Dialog {
        Dialog::Smart { id: Some(id.to_owned()), name: name.to_owned(), rules }
    }

    pub fn server() -> Dialog {
        Dialog::Server { name: String::new(), url: String::new(), user: String::new(), password: String::new() }
    }

    pub fn soloist(path: &str) -> Dialog {
        Dialog::Soloist { path: path.to_owned(), key: String::new() }
    }

    pub fn settings() -> Dialog {
        Dialog::Settings
    }
}

impl App {
    /// The tag editor for these songs. A field with different values starts empty.
    pub(crate) fn info_dialog(&self, ids: &[String]) -> Dialog {
        let Some(lib) = self.engine.library() else { return Dialog::About };
        let tracks: Vec<_> = ids.iter().filter_map(|id| lib.track(id)).collect();
        let value = |tag: Tag, t: &musiccraft_library::Track| -> String {
            let n = |v: u32| if v == 0 { String::new() } else { v.to_string() };
            match tag {
                Tag::Title => t.info.title.clone(),
                Tag::Artist => t.info.artist.clone(),
                Tag::AlbumArtist => t.info.album_artist.clone(),
                Tag::Album => t.info.album.clone(),
                Tag::Genre => t.info.genre.clone(),
                Tag::Composer => t.info.composer.clone(),
                Tag::Year => n(t.info.year),
                Tag::Track => n(t.info.track),
                Tag::Disc => n(t.info.disc),
            }
        };
        let fields = TAGS
            .into_iter()
            .map(|tag| {
                let mut all = tracks.iter().map(|t| value(tag, t));
                let first = all.next().unwrap_or_default();
                let same = all.all(|v| v == first);
                let first = if same { first } else { String::new() };
                (tag, Text { now: first.clone(), first })
            })
            .collect();
        let about = match tracks.as_slice() {
            [t] => format!(
                "{} · {} · {} kbps · {:.1} MB\n{}",
                t.kind(),
                time(t.info.duration_ms),
                t.info.bitrate,
                t.info.size as f64 / 1_000_000.0,
                t.info.path
            ),
            many => self.lang.n(many.len(), "{} song", "{} songs"),
        };
        Dialog::Info { ids: ids.to_vec(), fields, about }
    }
}

/// Draws the open dialog, if any.
pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(mut dialog) = app.dialog.take() else { return };
    let mut open = true;
    let modal = egui::Modal::new(egui::Id::unique("dialog")).show(ctx, |ui| {
        ui.set_min_width(380.0);
        ui.set_max_width(620.0);
        let before = std::mem::discriminant(&dialog);
        let keep = body(app, ui, &mut dialog);
        debug_assert_eq!(before, std::mem::discriminant(&dialog));
        keep
    });
    if !modal.inner || modal.should_close() {
        open = false;
    }
    if open && app.dialog.is_none() {
        app.dialog = Some(dialog);
    }
}

/// A text field with its label. The first field of a dialog takes the focus.
fn field(ui: &mut Ui, label: &str, text: &mut String, first: bool, password: bool) -> egui::Response {
    let l = ui.label(label);
    let edit = egui::TextEdit::singleline(text).desired_width(f32::INFINITY).password(password);
    let r = ui.add(edit).labelled_by(l.id);
    let key = ui.scope_id().with("focused-once");
    if first && !ui.data(|d| d.get_temp::<bool>(key).unwrap_or(false)) {
        ui.data_mut(|d| d.insert_temp(key, true));
        r.request_focus();
    }
    r
}

/// The buttons at the end of a dialog. Gives back (confirmed, cancelled).
fn buttons(app: &App, ui: &mut Ui, yes: &str, can: bool) -> (bool, bool) {
    let t = tokens(ui);
    ui.add_space(10.0);
    let mut out = (false, false);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let b = egui::Button::new(egui::RichText::new(yes).color(t.on_selection)).fill(t.selection);
        out.0 = ui.add_enabled(can, b).clicked();
        out.1 = ui.button(app.lang.t("Cancel")).clicked();
    });
    // Enter in a text field confirms.
    let enter = ui.input(|i| i.key_pressed(Key::Enter)) && ui.memory(|m| m.focused()).is_none();
    out.0 |= enter && can;
    out
}

/// Returns whether the dialog stays open.
fn body(app: &mut App, ui: &mut Ui, dialog: &mut Dialog) -> bool {
    let weak = tokens(ui).weak;
    match dialog {
        Dialog::NewPlaylist { name, items } => {
            ui.heading(app.lang.t("New Playlist"));
            field(ui, app.lang.t("Name"), name, true, false);
            let (yes, no) = buttons(app, ui, app.lang.t("Create"), !name.trim().is_empty());
            if yes {
                let items = std::mem::take(items);
                app.cmd(Command::PlaylistNew { name: name.trim().to_owned(), items });
            }
            !(yes || no)
        }
        Dialog::Rename { id, name } => {
            ui.heading(app.lang.t("Rename Playlist"));
            field(ui, app.lang.t("Name"), name, true, false);
            let (yes, no) = buttons(app, ui, app.lang.t("Rename"), !name.trim().is_empty());
            if yes {
                app.cmd(Command::PlaylistRename { id: id.clone(), name: name.trim().to_owned() });
            }
            !(yes || no)
        }
        Dialog::DeletePlaylist { id, name } => {
            ui.heading(app.lang.f("Delete “{}”?", &*name));
            ui.label(
                app.lang.t("The songs stay in the library. The playlist goes from every computer that shares it."),
            );
            let (yes, no) = buttons(app, ui, app.lang.t("Delete"), true);
            if yes {
                app.cmd(Command::PlaylistDelete { id: id.clone() });
            }
            !(yes || no)
        }
        Dialog::Remove { ids } => {
            let n = app.lang.n(ids.len(), "Remove {} song from the library?", "Remove {} songs from the library?");
            ui.heading(n);
            ui.label(
                app.lang.t("The files stay in the music folder. You can bring the songs back from the Edit menu."),
            );
            let (yes, no) = buttons(app, ui, app.lang.t("Remove"), true);
            if yes {
                app.cmd(Command::Remove { ids: ids.clone() });
            }
            !(yes || no)
        }
        Dialog::Smart { id, name, rules } => smart(app, ui, id.as_deref(), name, rules),
        Dialog::Info { ids, fields, about } => info(app, ui, ids, fields, about),
        Dialog::Station { name, url } => {
            ui.heading(app.lang.t("Add a Station"));
            field(ui, app.lang.t("Name"), name, true, false);
            field(ui, app.lang.t("Address of the stream or of its playlist"), url, false, false);
            let can = !name.trim().is_empty() && url.trim().starts_with("http");
            let (yes, no) = buttons(app, ui, app.lang.t("Add"), can);
            if yes {
                app.cmd(Command::StationAdd {
                    name: name.trim().to_owned(),
                    url: url.trim().to_owned(),
                    detail: String::new(),
                });
            }
            !(yes || no)
        }
        Dialog::Server { name, url, user, password } => {
            ui.heading(app.lang.t("Add a Music Server"));
            field(ui, app.lang.t("Address"), url, true, false);
            field(ui, app.lang.t("User"), user, false, false);
            field(ui, app.lang.t("Password"), password, false, true);
            field(ui, app.lang.t("Name to show"), name, false, false);
            if url.trim().starts_with("http://") {
                ui.label(
                    egui::RichText::new(app.lang.t("Without https, others on the network can see what you listen to."))
                        .color(tokens(ui).error),
                );
            }
            let can = !url.trim().is_empty() && !user.trim().is_empty();
            let (yes, no) = buttons(app, ui, app.lang.t("Add"), can);
            if yes {
                app.cmd(Command::ServerAdd {
                    name: name.clone(),
                    url: url.clone(),
                    user: user.clone(),
                    password: std::mem::take(password),
                });
            }
            !(yes || no)
        }
        Dialog::Soloist { path, key } => {
            ui.heading("Spotify Soloist");
            ui.label(app.lang.t(
                "Spotify's own player without a window. Download it from Spotify and create your own key there; MusicCraft starts it and stops it.",
            ));
            if ui.link("developer.spotify.com/documentation/soloist").clicked() {
                app.act(Action::Browse("https://developer.spotify.com/documentation/soloist".to_owned()));
            }
            field(ui, app.lang.t("Where the program is (empty: on the search path)"), path, true, false);
            field(ui, app.lang.t("Your key (empty: the one kept on this computer)"), key, false, true);
            let (yes, no) = buttons(app, ui, app.lang.t("Start"), true);
            if yes {
                app.cmd(Command::SoloistStart { path: path.clone(), key: std::mem::take(key) });
            }
            !(yes || no)
        }
        Dialog::Settings => settings(app, ui),
        Dialog::About => {
            ui.heading("MusicCraft");
            ui.label(app.lang.f("Version {}", env!("CARGO_PKG_VERSION")));
            ui.label(app.lang.t("A music library and player for your files, servers, radio and subscriptions."));
            ui.label(egui::RichText::new("© 2026 Mauro Mazzieri · MIT OR Apache-2.0").color(weak));
            ui.label(
                egui::RichText::new(
                    app.lang.t("An independent community app. Not affiliated with ArtCraft, Apple or Spotify."),
                )
                .color(weak),
            );
            ui.add_space(8.0);
            !ui.button(app.lang.t("Close")).clicked()
        }
    }
}

fn info(app: &mut App, ui: &mut Ui, ids: &[String], fields: &mut [(Tag, Text)], about: &str) -> bool {
    let weak = tokens(ui).weak;
    ui.heading(app.lang.t("Song Information"));
    ui.label(egui::RichText::new(about).color(weak));
    ui.add_space(6.0);
    let many = ids.len() > 1;
    egui::Grid::new("tags").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        for (tag, text) in fields.iter_mut() {
            // A title or a number for many songs at once is never meant.
            if many && matches!(tag, Tag::Title | Tag::Track) {
                continue;
            }
            let name = match tag {
                Tag::Title => app.lang.t("Title"),
                Tag::Artist => app.lang.t("Artist"),
                Tag::AlbumArtist => app.lang.t("Album artist"),
                Tag::Album => app.lang.t("Album"),
                Tag::Genre => app.lang.t("Genre"),
                Tag::Composer => app.lang.t("Composer"),
                Tag::Year => app.lang.t("Year"),
                Tag::Track => app.lang.t("Track number"),
                Tag::Disc => app.lang.t("Disc number"),
            };
            let l = ui.label(name);
            let hint = if many && text.first.is_empty() { app.lang.t("(left as it is)") } else { "" };
            ui.add(egui::TextEdit::singleline(&mut text.now).hint_text(hint).desired_width(360.0)).labelled_by(l.id);
            ui.end_row();
        }
    });
    ui.label(egui::RichText::new(app.lang.t("The tags are written into the files.")).color(weak));
    let changed = fields.iter().any(|(_, t)| t.first != t.now);
    let numbers_ok = fields
        .iter()
        .filter(|(tag, _)| matches!(tag, Tag::Year | Tag::Track | Tag::Disc))
        .all(|(_, t)| t.now.trim().is_empty() || t.now.trim().parse::<u32>().is_ok_and(|n| n < 10_000));
    if !numbers_ok {
        ui.label(egui::RichText::new(app.lang.t("Year and numbers take digits only.")).color(tokens(ui).error));
    }
    let (yes, no) = buttons(app, ui, app.lang.t("Save"), changed && numbers_ok);
    if yes {
        let mut e = TagEdit::default();
        for (tag, t) in fields.iter().filter(|(_, t)| t.first != t.now) {
            let s = Some(t.now.clone());
            let n = Some(t.now.trim().parse().unwrap_or(0));
            match tag {
                Tag::Title => e.title = s,
                Tag::Artist => e.artist = s,
                Tag::AlbumArtist => e.album_artist = s,
                Tag::Album => e.album = s,
                Tag::Genre => e.genre = s,
                Tag::Composer => e.composer = s,
                Tag::Year => e.year = n,
                Tag::Track => e.track = n,
                Tag::Disc => e.disc = n,
            }
        }
        app.cmd(Command::Edit { ids: ids.to_vec(), tags: e });
    }
    !(yes || no)
}

fn field_name(l: &Lang, f: Field) -> &'static str {
    match f {
        Field::Title => l.t("Title"),
        Field::Artist => l.t("Artist"),
        Field::Album => l.t("Album"),
        Field::Genre => l.t("Genre"),
        Field::Composer => l.t("Composer"),
        Field::Year => l.t("Year"),
        Field::Rating => l.t("Rating"),
        Field::Loved => l.t("Love (1 or 0)"),
        Field::Plays => l.t("Plays"),
        Field::AddedDays => l.t("Added, days ago"),
        Field::PlayedDays => l.t("Played, days ago"),
        Field::Minutes => l.t("Minutes"),
    }
}

fn op_name(l: &Lang, op: Op) -> &'static str {
    match op {
        Op::Is => l.t("is"),
        Op::IsNot => l.t("is not"),
        Op::Contains => l.t("contains"),
        Op::Over => l.t("is more than"),
        Op::Under => l.t("is less than"),
    }
}

fn smart(app: &mut App, ui: &mut Ui, id: Option<&str>, name: &mut String, rules: &mut Rules) -> bool {
    ui.heading(if id.is_some() { app.lang.t("Edit Rules") } else { app.lang.t("New Smart Playlist") });
    if id.is_none() {
        field(ui, app.lang.t("Name"), name, true, false);
    }
    ui.horizontal(|ui| {
        ui.label(app.lang.t("Songs that match"));
        ui.radio_value(&mut rules.all, true, app.lang.t("all rules"));
        ui.radio_value(&mut rules.all, false, app.lang.t("any rule"));
    });
    let mut drop = None;
    for (i, r) in rules.rules.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            ui.horizontal(|ui| {
                let was_text = r.field.is_text();
                egui::ComboBox::from_id_salt("field")
                    .width(150.0)
                    .selected_text(field_name(&app.lang, r.field))
                    .show_ui(ui, |ui| {
                        for f in FIELDS {
                            ui.selectable_value(&mut r.field, f, field_name(&app.lang, f));
                        }
                    });
                if r.field.is_text() != was_text {
                    r.op = if r.field.is_text() { Op::Contains } else { Op::Over };
                }
                let ops: &[Op] = if r.field.is_text() {
                    &[Op::Contains, Op::Is, Op::IsNot]
                } else {
                    &[Op::Is, Op::IsNot, Op::Over, Op::Under]
                };
                egui::ComboBox::from_id_salt("op").width(110.0).selected_text(op_name(&app.lang, r.op)).show_ui(
                    ui,
                    |ui| {
                        for op in ops {
                            ui.selectable_value(&mut r.op, *op, op_name(&app.lang, *op));
                        }
                    },
                );
                let value = ui.add(egui::TextEdit::singleline(&mut r.value).desired_width(140.0));
                let label = app.lang.t("Value");
                let text = r.value.clone();
                value.widget_info(|| {
                    let mut i = egui::WidgetInfo::labeled(egui::Role::TextInput, true, label);
                    i.current_text_value = Some(text.clone());
                    i
                });
                if ui.button(app.lang.t("Remove")).clicked() {
                    drop = Some(i);
                }
            });
        });
    }
    if let Some(i) = drop {
        rules.rules.remove(i);
    }
    if ui.button(app.lang.t("Add a Rule")).clicked() {
        rules.rules.push(Rule { field: Field::Artist, op: Op::Contains, value: String::new() });
    }
    ui.separator();
    ui.horizontal(|ui| {
        let l = ui.label(app.lang.t("At most (0: no limit)"));
        ui.add(egui::DragValue::new(&mut rules.limit).range(0..=100_000)).labelled_by(l.id);
        ui.label(app.lang.t("songs, by"));
        let sorts = [
            (Sort::Artist, app.lang.t("Artist")),
            (Sort::Title, app.lang.t("Title")),
            (Sort::Plays, app.lang.t("Plays")),
            (Sort::Rating, app.lang.t("Rating")),
            (Sort::Added, app.lang.t("Added")),
            (Sort::LastPlayed, app.lang.t("Last played")),
            (Sort::Year, app.lang.t("Year")),
        ];
        let shown = sorts.iter().find(|(s, _)| *s == rules.sort).map_or("", |(_, n)| n);
        egui::ComboBox::from_id_salt("sort").selected_text(shown).show_ui(ui, |ui| {
            for (s, n) in sorts {
                ui.selectable_value(&mut rules.sort, s, n);
            }
        });
        ui.checkbox(&mut rules.descending, app.lang.t("greatest first"));
    });
    let can = !rules.rules.is_empty() && (id.is_some() || !name.trim().is_empty());
    let (yes, no) = buttons(app, ui, app.lang.t("Save"), can);
    if yes {
        match id {
            Some(id) => app.cmd(Command::SmartSet { id: id.to_owned(), rules: rules.clone() }),
            None => app.cmd(Command::SmartNew { name: name.trim().to_owned(), rules: rules.clone() }),
        }
    }
    !(yes || no)
}

fn settings(app: &mut App, ui: &mut Ui) -> bool {
    let weak = tokens(ui).weak;
    ui.heading(app.lang.t("Settings"));
    ui.add_space(6.0);
    ui.label(app.lang.t("Library folder"));
    let folder = app.engine.settings.library.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(folder).monospace());
        if ui.button(app.lang.t("Change…")).clicked() {
            app.act(Action::ChooseLibrary);
        }
    });
    let note = app.lang.t(
        "To share it, put the folder in OneDrive, Google Drive or a similar service and choose it on each computer. Songs that are only in the cloud are listed, and downloaded when played.",
    );
    ui.label(egui::RichText::new(note).color(weak));
    if let Some(lib) = app.engine.library() {
        let computers = lib.devices().join(", ");
        ui.label(egui::RichText::new(app.lang.f("Computers that share it: {}", computers)).color(weak));
        for w in lib.warnings().iter().take(3) {
            ui.label(egui::RichText::new(w).color(tokens(ui).error));
        }
    }
    for (path, why) in app.engine.unreadable.iter().take(3) {
        ui.label(egui::RichText::new(format!("{path}: {why}")).color(tokens(ui).error));
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let l = ui.label(app.lang.t("Language"));
        let mut code = app.engine.settings.language.clone();
        let system = app.lang.t("System");
        let shown = LANGUAGES.iter().find(|(c, _)| *c == code).map_or(system, |(_, n)| n);
        let combo = egui::ComboBox::from_id_salt("language").selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(&mut code, String::new(), system);
            for (c, n) in LANGUAGES {
                ui.selectable_value(&mut code, c.to_owned(), n);
            }
        });
        combo.response.labelled_by(l.id);
        if code != app.engine.settings.language {
            app.lang = std::rc::Rc::new(Lang::new(&code));
            app.engine.change_settings(|s| s.language = code);
        }
    });
    ui.horizontal(|ui| {
        ui.label(app.lang.t("Appearance"));
        let mut theme = app.engine.settings.theme;
        ui.radio_value(&mut theme, Theme::System, app.lang.t("System"));
        ui.radio_value(&mut theme, Theme::Light, app.lang.t("Light"));
        ui.radio_value(&mut theme, Theme::Dark, app.lang.t("Dark"));
        if theme != app.engine.settings.theme {
            set_theme(ui.ctx(), theme);
            app.engine.change_settings(|s| s.theme = theme);
        }
    });
    ui.add_space(10.0);
    !ui.button(app.lang.t("Close")).clicked()
}
