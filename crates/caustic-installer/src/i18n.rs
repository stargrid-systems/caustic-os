#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
}

impl Lang {
    pub fn detect() -> Self {
        for var in &["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(val) = std::env::var(var) {
                if val.to_lowercase().starts_with("de") {
                    return Self::De;
                }
            }
        }
        Self::En
    }
}

#[derive(Clone, Copy)]
pub enum Text {
    Loading,
    SelectRelease,
    Download,
    Downloading,
    SelectDisk,
    Flash,
    Flashing,
    Done,
    DoneHint,
    Error,
    RpibootNotFound,
}

pub fn t(lang: Lang, text: Text) -> &'static str {
    match (lang, text) {
        (Lang::En, Text::Loading) => "Loading available releases...",
        (Lang::De, Text::Loading) => "Verfuegbare Releases werden geladen...",

        (Lang::En, Text::SelectRelease) => "Select a release",
        (Lang::De, Text::SelectRelease) => "Release auswaehlen",

        (Lang::En, Text::Download) => "Download",
        (Lang::De, Text::Download) => "Herunterladen",

        (Lang::En, Text::Downloading) => "Downloading image...",
        (Lang::De, Text::Downloading) => "Image wird heruntergeladen...",

        (Lang::En, Text::SelectDisk) => "Select target disk",
        (Lang::De, Text::SelectDisk) => "Ziel-Datentraeger auswaehlen",

        (Lang::En, Text::Flash) => "Flash image",
        (Lang::De, Text::Flash) => "Image flashen",

        (Lang::En, Text::Flashing) => "Flashing image to disk...",
        (Lang::De, Text::Flashing) => "Image wird auf Datentraeger geflasht...",

        (Lang::En, Text::Done) => "Installation complete!",
        (Lang::De, Text::Done) => "Installation abgeschlossen!",

        (Lang::En, Text::DoneHint) => "You can now boot the device.",
        (Lang::De, Text::DoneHint) => "Sie koennen das Geraet jetzt starten.",

        (Lang::En, Text::Error) => "Error",
        (Lang::De, Text::Error) => "Fehler",

        (Lang::En, Text::RpibootNotFound) => {
            "rpiboot was not found on this system.\n\
             Install it from https://github.com/raspberrypi/usbboot"
        }
        (Lang::De, Text::RpibootNotFound) => {
            "rpiboot wurde auf diesem System nicht gefunden.\n\
             Installieren Sie es von https://github.com/raspberrypi/usbboot"
        }
    }
}
