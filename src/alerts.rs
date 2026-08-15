use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const BUILTIN_PACK_ID: &str = "builtin";
const BUILTIN_SOUND_RESOURCE: &str = "/dev/omp/Native/sounds/confirmation-001.ogg";
const NOTIFICATION_ID: &str = "agent-status";
const SOUND_DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlertKind {
    Idle,
    GoalComplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowStatus {
    Ready,
    Working,
    GoalComplete,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum SoundEvent {
    SessionStart,
    TaskAcknowledge,
    TaskComplete,
    TaskError,
    InputRequired,
    ResourceLimit,
    UserSpam,
    SessionEnd,
    TaskProgress,
}

impl SoundEvent {
    pub const ALL: [Self; 9] = [
        Self::SessionStart,
        Self::TaskAcknowledge,
        Self::TaskComplete,
        Self::TaskError,
        Self::InputRequired,
        Self::ResourceLimit,
        Self::UserSpam,
        Self::SessionEnd,
        Self::TaskProgress,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::SessionStart => "session.start",
            Self::TaskAcknowledge => "task.acknowledge",
            Self::TaskComplete => "task.complete",
            Self::TaskError => "task.error",
            Self::InputRequired => "input.required",
            Self::ResourceLimit => "resource.limit",
            Self::UserSpam => "user.spam",
            Self::SessionEnd => "session.end",
            Self::TaskProgress => "task.progress",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::SessionStart => "Session starts",
            Self::TaskAcknowledge => "Work begins",
            Self::TaskComplete => "Work completes",
            Self::TaskError => "Task fails",
            Self::InputRequired => "Input is needed",
            Self::ResourceLimit => "Usage limit is reached",
            Self::UserSpam => "Prompts are sent rapidly",
            Self::SessionEnd => "Session ends",
            Self::TaskProgress => "Long task continues",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::SessionStart => "When omp opens or starts a new conversation",
            Self::TaskAcknowledge => "When an agent accepts a prompt and starts working",
            Self::TaskComplete => "When an agent becomes ready or completes a goal",
            Self::TaskError => "When a tool or agent fails",
            Self::InputRequired => "When omp is waiting for a choice or confirmation",
            Self::ResourceLimit => "When a rate, token, quota, or credit limit is reached",
            Self::UserSpam => "After several prompts are sent in quick succession",
            Self::SessionEnd => "When a conversation or the app closes",
            Self::TaskProgress => "Every 30 seconds while an agent is still working",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AlertPreferences {
    #[serde(default = "enabled")]
    pub desktop_notifications: bool,
    #[serde(default = "enabled")]
    pub sounds: bool,
    #[serde(default = "default_volume")]
    pub volume: f64,
    #[serde(default)]
    pub event_packs: HashMap<String, String>,
}

impl Default for AlertPreferences {
    fn default() -> Self {
        Self {
            desktop_notifications: true,
            sounds: true,
            volume: default_volume(),
            event_packs: HashMap::from([(
                SoundEvent::TaskComplete.key().to_owned(),
                BUILTIN_PACK_ID.to_owned(),
            )]),
        }
    }
}

impl AlertPreferences {
    pub fn pack_for(&self, event: SoundEvent) -> Option<&str> {
        self.event_packs.get(event.key()).map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SoundPackChoice {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone)]
enum SoundSource {
    Resource(&'static str),
    File(PathBuf),
}

#[derive(Default)]
struct AudioPlayer {
    output: Option<rodio::MixerDeviceSink>,
    current: Option<rodio::Player>,
}

impl AudioPlayer {
    fn play(&mut self, source: &SoundSource, volume: f64) -> Result<(), String> {
        if self.output.is_none() {
            let mut output = rodio::DeviceSinkBuilder::open_default_sink()
                .map_err(|error| format!("Could not open the audio output: {error}"))?;
            output.log_on_drop(false);
            self.output = Some(output);
        }
        let output = self.output.as_ref().expect("audio output was initialized");
        let player = rodio::Player::connect_new(output.mixer());
        player.set_volume(volume.clamp(0.0, 1.0) as f32);

        match source {
            SoundSource::Resource(resource) => {
                let bytes = gio::resources_lookup_data(resource, gio::ResourceLookupFlags::NONE)
                    .map_err(|error| {
                        format!("Could not load sound resource {resource}: {error}")
                    })?;
                let decoder = rodio::Decoder::try_from(Cursor::new(bytes)).map_err(|error| {
                    format!("Could not decode sound resource {resource}: {error}")
                })?;
                player.append(decoder);
            }
            SoundSource::File(path) => {
                let file = fs::File::open(path)
                    .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
                let decoder = rodio::Decoder::try_from(file)
                    .map_err(|error| format!("Could not decode {}: {error}", path.display()))?;
                player.append(decoder);
            }
        }

        self.current = Some(player);
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct SoundPack {
    id: String,
    name: String,
    label: String,
    root: Option<PathBuf>,
    categories: HashMap<SoundEvent, Vec<SoundSource>>,
}

impl SoundPack {
    fn builtin() -> Self {
        Self {
            id: BUILTIN_PACK_ID.to_owned(),
            name: BUILTIN_PACK_ID.to_owned(),
            label: "Default".to_owned(),
            root: None,
            categories: HashMap::from([(
                SoundEvent::TaskComplete,
                vec![SoundSource::Resource(BUILTIN_SOUND_RESOURCE)],
            )]),
        }
    }

    fn supports(&self, event: SoundEvent) -> bool {
        self.categories
            .get(&event)
            .is_some_and(|sounds| !sounds.is_empty())
    }
}

pub(crate) struct Alerts {
    application: adw::Application,
    preferences: RefCell<AlertPreferences>,
    packs: RefCell<Vec<SoundPack>>,
    next_sounds: RefCell<HashMap<(String, SoundEvent), usize>>,
    last_events: RefCell<HashMap<SoundEvent, Instant>>,
    audio: RefCell<AudioPlayer>,
}

impl Alerts {
    pub fn new(application: &adw::Application) -> Self {
        let packs = discover_sound_packs();
        let preferences = normalize_preferences(load_preferences(), &packs);
        Self {
            application: application.clone(),
            preferences: RefCell::new(preferences),
            packs: RefCell::new(packs),
            next_sounds: RefCell::new(HashMap::new()),
            last_events: RefCell::new(HashMap::new()),
            audio: RefCell::new(AudioPlayer::default()),
        }
    }

    pub fn preferences(&self) -> AlertPreferences {
        self.preferences.borrow().clone()
    }

    pub fn sound_pack_choices(&self, event: SoundEvent) -> Vec<SoundPackChoice> {
        self.packs
            .borrow()
            .iter()
            .filter(|pack| pack.supports(event))
            .map(|pack| SoundPackChoice {
                id: pack.id.clone(),
                label: pack.label.clone(),
            })
            .collect()
    }

    pub fn installed_pack_names(&self) -> HashSet<String> {
        self.packs
            .borrow()
            .iter()
            .filter(|pack| pack.id != BUILTIN_PACK_ID)
            .map(|pack| pack.name.clone())
            .collect()
    }

    pub fn installed_pack_count(&self) -> usize {
        self.packs
            .borrow()
            .iter()
            .filter(|pack| pack.id != BUILTIN_PACK_ID)
            .count()
    }

    pub fn refresh_sound_packs(&self) {
        let packs = discover_sound_packs();
        let preferences = normalize_preferences(self.preferences.borrow().clone(), &packs);
        self.packs.replace(packs);
        self.preferences.replace(preferences);
        self.next_sounds.borrow_mut().clear();
    }

    pub fn set_desktop_notifications(&self, enabled: bool) -> Result<(), String> {
        self.update_preferences(|preferences| preferences.desktop_notifications = enabled)
    }

    pub fn set_sounds(&self, enabled: bool) -> Result<(), String> {
        self.update_preferences(|preferences| preferences.sounds = enabled)
    }

    pub fn set_volume(&self, volume: f64) -> Result<(), String> {
        let volume = volume.clamp(0.0, 1.0);
        self.update_preferences(|preferences| preferences.volume = volume)
    }

    pub fn set_event_pack(&self, event: SoundEvent, pack_id: Option<&str>) -> Result<(), String> {
        if let Some(pack_id) = pack_id {
            let valid = self
                .packs
                .borrow()
                .iter()
                .any(|pack| pack.id == pack_id && pack.supports(event));
            if !valid {
                return Err("The selected pack does not contain a sound for this event".to_owned());
            }
        }
        self.next_sounds
            .borrow_mut()
            .retain(|(_, candidate), _| *candidate != event);
        self.update_preferences(|preferences| match pack_id {
            Some(pack_id) => {
                preferences
                    .event_packs
                    .insert(event.key().to_owned(), pack_id.to_owned());
            }
            None => {
                preferences.event_packs.remove(event.key());
            }
        })
    }

    pub fn play(&self, event: SoundEvent) {
        let preferences = self.preferences.borrow().clone();
        if !preferences.sounds {
            return;
        }
        let Some(pack_id) = preferences.pack_for(event) else {
            return;
        };
        let now = Instant::now();
        if self
            .last_events
            .borrow()
            .get(&event)
            .is_some_and(|last| now.duration_since(*last) < SOUND_DEBOUNCE)
        {
            return;
        }
        if self.play_from_pack(event, pack_id, preferences.volume) {
            self.last_events.borrow_mut().insert(event, now);
        }
    }

    pub fn preview(&self, event: SoundEvent, pack_id: &str) {
        self.play_from_pack(event, pack_id, self.preferences.borrow().volume);
    }

    pub fn notify(&self, kind: AlertKind, session_title: &str, window_is_background: bool) {
        if !self.preferences.borrow().desktop_notifications || !window_is_background {
            return;
        }
        let (title, body, priority) = match kind {
            AlertKind::Idle => (
                "Agent is ready",
                format!("{session_title} is waiting for your next message."),
                gio::NotificationPriority::Normal,
            ),
            AlertKind::GoalComplete => (
                "Goal completed",
                format!("{session_title} completed its goal."),
                gio::NotificationPriority::High,
            ),
        };
        let notification = gio::Notification::new(title);
        notification.set_body(Some(&body));
        notification.set_priority(priority);
        notification.set_default_action("app.present");
        self.application
            .send_notification(Some(NOTIFICATION_ID), &notification);
    }

    pub fn withdraw(&self) {
        self.application.withdraw_notification(NOTIFICATION_ID);
    }

    fn update_preferences(&self, update: impl FnOnce(&mut AlertPreferences)) -> Result<(), String> {
        let mut preferences = self.preferences.borrow().clone();
        update(&mut preferences);
        save_preferences(&preferences)?;
        self.preferences.replace(preferences);
        Ok(())
    }

    fn play_from_pack(&self, event: SoundEvent, pack_id: &str, volume: f64) -> bool {
        let packs = self.packs.borrow();
        let Some(pack) = packs.iter().find(|pack| pack.id == pack_id) else {
            return false;
        };
        let Some(sounds) = pack
            .categories
            .get(&event)
            .filter(|sounds| !sounds.is_empty())
        else {
            return false;
        };
        let key = (pack.id.clone(), event);
        let index = {
            let mut next_sounds = self.next_sounds.borrow_mut();
            let index = next_sounds.get(&key).copied().unwrap_or(0) % sounds.len();
            next_sounds.insert(key, index + 1);
            index
        };
        match self.audio.borrow_mut().play(&sounds[index], volume) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("{error}");
                false
            }
        }
    }
}

pub(crate) fn is_goal_completion(tool_name: &str, arguments: &Value) -> bool {
    tool_name == "goal" && arguments.get("op").and_then(Value::as_str) == Some("complete")
}

pub(crate) fn alert_for_agent_end(was_running: bool, goal_completed: bool) -> Option<AlertKind> {
    (was_running && !goal_completed).then_some(AlertKind::Idle)
}

pub(crate) fn sound_event_for_error(message: &str) -> SoundEvent {
    let message = message.to_ascii_lowercase();
    if [
        "rate limit",
        "resource limit",
        "usage limit",
        "token limit",
        "quota",
        "credits exhausted",
        "out of credits",
    ]
    .iter()
    .any(|marker| message.contains(marker))
    {
        SoundEvent::ResourceLimit
    } else {
        SoundEvent::TaskError
    }
}

pub(crate) fn window_title(status: WindowStatus, session_title: &str) -> String {
    let marker = match status {
        WindowStatus::Ready => "Ready",
        WindowStatus::Working => "Working",
        WindowStatus::GoalComplete => "Goal completed",
        WindowStatus::Disconnected => "Disconnected",
    };
    format!("{marker} · {session_title} — omp")
}

pub(crate) fn managed_packs_dir() -> PathBuf {
    glib::user_data_dir().join("omp-native").join("sound-packs")
}

fn enabled() -> bool {
    true
}

fn default_volume() -> f64 {
    0.8
}

fn preferences_path() -> PathBuf {
    glib::user_config_dir()
        .join("omp-native")
        .join("preferences.json")
}

fn load_preferences() -> AlertPreferences {
    let path = preferences_path();
    match fs::read(&path) {
        Ok(contents) => decode_preferences(&contents).unwrap_or_else(|error| {
            eprintln!(
                "Could not read alert preferences from {}: {error}",
                path.display()
            );
            AlertPreferences::default()
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => AlertPreferences::default(),
        Err(error) => {
            eprintln!(
                "Could not read alert preferences from {}: {error}",
                path.display()
            );
            AlertPreferences::default()
        }
    }
}

fn decode_preferences(contents: &[u8]) -> Result<AlertPreferences, serde_json::Error> {
    let value = serde_json::from_slice::<Value>(contents)?;
    let legacy_pack = value
        .get("sound_pack")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let mut preferences = serde_json::from_value::<AlertPreferences>(value)?;
    if preferences.event_packs.is_empty()
        && let Some(pack) = legacy_pack
    {
        preferences
            .event_packs
            .insert(SoundEvent::TaskComplete.key().to_owned(), pack);
    }
    preferences.volume = preferences.volume.clamp(0.0, 1.0);
    Ok(preferences)
}

fn normalize_preferences(
    mut preferences: AlertPreferences,
    packs: &[SoundPack],
) -> AlertPreferences {
    for pack_id in preferences.event_packs.values_mut() {
        if packs.iter().any(|pack| pack.id == *pack_id) {
            continue;
        }
        if let Some(pack) = packs.iter().find(|pack| {
            pack.root
                .as_deref()
                .is_some_and(|root| root.to_string_lossy() == pack_id.as_str())
        }) {
            *pack_id = pack.id.clone();
        }
    }
    preferences
}

fn save_preferences(preferences: &AlertPreferences) -> Result<(), String> {
    let path = preferences_path();
    let parent = path
        .parent()
        .ok_or_else(|| "Alert preferences have no parent directory".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
    let temporary = path.with_extension("json.tmp");
    let contents = serde_json::to_vec_pretty(preferences)
        .map_err(|error| format!("Could not encode alert preferences: {error}"))?;
    fs::write(&temporary, contents)
        .map_err(|error| format!("Could not write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, &path)
        .map_err(|error| format!("Could not replace {}: {error}", path.display()))
}

fn discover_sound_packs() -> Vec<SoundPack> {
    let home = glib::home_dir();
    let roots = [
        managed_packs_dir(),
        home.join(".openpeon/packs"),
        home.join(".peon-ping/packs"),
        home.join(".claude/hooks/peon-ping/packs"),
    ];
    let mut packs = vec![SoundPack::builtin()];
    let mut names = HashSet::new();
    for root in roots {
        for entry in fs::read_dir(root)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            let Some(pack) = load_sound_pack(&entry.path()) else {
                continue;
            };
            if names.insert(pack.name.clone()) {
                packs.push(pack);
            }
        }
    }
    packs[1..].sort_by(|left, right| left.label.cmp(&right.label).then(left.id.cmp(&right.id)));
    packs
}

fn load_sound_pack(root: &Path) -> Option<SoundPack> {
    let manifest_path = root.join("openpeon.json");
    let contents = fs::read_to_string(manifest_path).ok()?;
    parse_sound_pack(root, &contents)
}

#[derive(Deserialize)]
struct PackManifest {
    #[serde(default)]
    cesp_version: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    display_name: String,
    categories: HashMap<String, PackCategory>,
    #[serde(default)]
    category_aliases: HashMap<String, String>,
}

#[derive(Deserialize)]
struct PackCategory {
    sounds: Vec<PackSound>,
}

#[derive(Deserialize)]
struct PackSound {
    file: PathBuf,
}

fn parse_sound_pack(root: &Path, manifest: &str) -> Option<SoundPack> {
    let manifest = serde_json::from_str::<PackManifest>(manifest).ok()?;
    if manifest
        .cesp_version
        .as_deref()
        .is_some_and(|version| version != "1.0")
    {
        return None;
    }
    let mut categories = HashMap::new();
    for event in SoundEvent::ALL {
        let category = manifest.categories.get(event.key()).or_else(|| {
            manifest
                .category_aliases
                .iter()
                .find(|(_, target)| target.as_str() == event.key())
                .and_then(|(alias, _)| manifest.categories.get(alias))
        });
        let sounds = category
            .into_iter()
            .flat_map(|category| &category.sounds)
            .filter_map(|sound| normalized_sound_path(&sound.file))
            .map(|path| root.join(path))
            .filter(|path| path.is_file())
            .map(SoundSource::File)
            .collect::<Vec<_>>();
        if !sounds.is_empty() {
            categories.insert(event, sounds);
        }
    }
    if categories.is_empty() {
        return None;
    }
    let name = if manifest.name.trim().is_empty() {
        root.file_name()?.to_string_lossy().into_owned()
    } else {
        manifest.name
    };
    let label = if manifest.display_name.trim().is_empty() {
        name.clone()
    } else {
        manifest.display_name
    };
    Some(SoundPack {
        id: format!("pack:{name}"),
        name,
        label,
        root: Some(root.to_owned()),
        categories,
    })
}

fn normalized_sound_path(path: &Path) -> Option<PathBuf> {
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    if path.components().count() == 1 {
        Some(Path::new("sounds").join(path))
    } else {
        Some(path.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_every_standard_sound_event_once() {
        let keys = SoundEvent::ALL
            .iter()
            .map(|event| event.key())
            .collect::<HashSet<_>>();
        assert_eq!(SoundEvent::ALL.len(), 9);
        assert_eq!(keys.len(), SoundEvent::ALL.len());
    }

    #[test]
    fn loads_per_event_categories_and_standard_aliases() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "omp-native-sound-pack-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("sounds")).expect("create sound fixture");
        fs::write(root.join("sounds/done.ogg"), b"OggSdone").expect("write completion sound");
        fs::write(root.join("sounds/start.ogg"), b"OggSstart").expect("write start sound");
        let manifest = r#"{
            "cesp_version": "1.0",
            "name": "fixture",
            "display_name": "Fixture Pack",
            "categories": {
                "task.complete": {"sounds": [{"file": "done.ogg"}]},
                "legacy.start": {"sounds": [{"file": "start.ogg"}]}
            },
            "category_aliases": {"legacy.start": "session.start"}
        }"#;
        let pack = parse_sound_pack(&root, manifest).expect("parse sound pack");
        assert_eq!(pack.id, "pack:fixture");
        assert_eq!(pack.label, "Fixture Pack");
        assert!(pack.supports(SoundEvent::TaskComplete));
        assert!(pack.supports(SoundEvent::SessionStart));
        assert!(!pack.supports(SoundEvent::TaskError));
        fs::remove_dir_all(root).expect("remove sound fixture");
    }

    #[test]
    fn migrates_the_previous_completion_pack_preference() {
        let preferences = decode_preferences(
            br#"{"desktop_notifications":true,"sounds":true,"sound_pack":"pack:peon"}"#,
        )
        .expect("legacy preferences decode");
        assert_eq!(
            preferences.pack_for(SoundEvent::TaskComplete),
            Some("pack:peon")
        );
        assert_eq!(preferences.pack_for(SoundEvent::SessionStart), None);
    }

    #[test]
    fn recognizes_only_goal_completion_calls() {
        assert!(is_goal_completion(
            "goal",
            &serde_json::json!({ "op": "complete" })
        ));
        assert!(!is_goal_completion(
            "goal",
            &serde_json::json!({ "op": "get" })
        ));
        assert!(!is_goal_completion(
            "todo",
            &serde_json::json!({ "op": "complete" })
        ));
    }

    #[test]
    fn idle_alert_requires_a_finished_run_without_goal_completion() {
        assert_eq!(alert_for_agent_end(true, false), Some(AlertKind::Idle));
        assert_eq!(alert_for_agent_end(false, false), None);
        assert_eq!(alert_for_agent_end(true, true), None);
    }

    #[test]
    fn distinguishes_usage_limits_from_other_errors() {
        assert_eq!(
            sound_event_for_error("Provider rate limit reached"),
            SoundEvent::ResourceLimit
        );
        assert_eq!(
            sound_event_for_error("Tool returned exit code 1"),
            SoundEvent::TaskError
        );
    }

    #[test]
    fn formats_window_state_with_the_session_title() {
        assert_eq!(
            window_title(WindowStatus::Working, "Native alerts"),
            "Working · Native alerts — omp"
        );
        assert_eq!(
            window_title(WindowStatus::GoalComplete, "Native alerts"),
            "Goal completed · Native alerts — omp"
        );
    }

    #[test]
    fn rejects_pack_paths_that_escape_the_pack_directory() {
        assert_eq!(
            normalized_sound_path(Path::new("done.ogg")),
            Some(PathBuf::from("sounds/done.ogg"))
        );
        assert_eq!(
            normalized_sound_path(Path::new("sounds/done.ogg")),
            Some(PathBuf::from("sounds/done.ogg"))
        );
        assert_eq!(normalized_sound_path(Path::new("../done.ogg")), None);
        assert_eq!(normalized_sound_path(Path::new("/tmp/done.ogg")), None);
    }
}
