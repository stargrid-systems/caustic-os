#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
}

impl Lang {
    pub fn detect() -> Self {
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(val) = std::env::var(var)
                && val.to_lowercase().starts_with("de")
            {
                return Self::De;
            }
        }
        Self::En
    }
}

#[derive(Clone, Copy)]
pub enum Text {
    Loading,
    SelectRelease,
    Production,
    Development,
    Download,
    Downloading,
    SelectDisk,
    DataLossWarning,
    Flash,
    Flashing,
    Done,
    DoneHint,
    Close,
    Error,
    RpibootNotFound,
}

pub const fn t(lang: Lang, text: Text) -> &'static str {
    match (lang, text) {
        (Lang::En, Text::Loading) => "Loading available releases...",
        (Lang::De, Text::Loading) => "Verfügbare Releases werden geladen...",

        (Lang::En, Text::SelectRelease) => "Select a release",
        (Lang::De, Text::SelectRelease) => "Release auswählen",

        (Lang::En, Text::Production) => "Production",
        (Lang::De, Text::Production) => "Produktion",

        (Lang::En, Text::Development) => "Development",
        (Lang::De, Text::Development) => "Entwicklung",

        (Lang::En, Text::Download) => "Download",
        (Lang::De, Text::Download) => "Herunterladen",

        (Lang::En, Text::Downloading) => "Downloading image...",
        (Lang::De, Text::Downloading) => "Image wird heruntergeladen...",

        (Lang::En, Text::SelectDisk) => "Select target disk",
        (Lang::De, Text::SelectDisk) => "Zieldatenträger auswählen",

        (Lang::En, Text::DataLossWarning) => "All data on the selected disk will be erased!",
        (Lang::De, Text::DataLossWarning) => {
            "Alle Daten auf dem ausgewählten Datenträger werden gelöscht!"
        }

        (Lang::En, Text::Flash) => "Flash image",
        (Lang::De, Text::Flash) => "Image flashen",

        (Lang::En, Text::Flashing) => "Flashing image to disk...",
        (Lang::De, Text::Flashing) => "Image wird auf den Datenträger geflasht...",

        (Lang::En, Text::Done) => "Installation complete!",
        (Lang::De, Text::Done) => "Installation abgeschlossen!",

        (Lang::En, Text::DoneHint) => "You can now boot the device.",
        (Lang::De, Text::DoneHint) => "Das Gerät kann jetzt gestartet werden.",

        (Lang::En, Text::Close) => "Close",
        (Lang::De, Text::Close) => "Schließen",

        (Lang::En, Text::Error) => "Error",
        (Lang::De, Text::Error) => "Fehler",

        (Lang::En, Text::RpibootNotFound) => {
            "rpiboot was not found on this system.\n\
             Install it from https://github.com/raspberrypi/usbboot"
        }
        (Lang::De, Text::RpibootNotFound) => {
            "rpiboot wurde auf diesem System nicht gefunden.\n\
             Zu finden unter https://github.com/raspberrypi/usbboot"
        }
    }
}
