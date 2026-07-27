use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use iced::task::{sipper, Straw};
use iced::widget::{
    button, column, container, progress_bar, row, scrollable, text, Space,
};
use iced::{Center, Element, Fill, Task};

use crate::disk::{self, Disk};
use crate::flash;
use crate::i18n::{t, Lang, Text};
use crate::usbboot;

const REGISTRY_PROD: &str = "ghcr.io/stargrid-systems/caustic-os";
const REGISTRY_DEV: &str = "ghcr.io/stargrid-systems/caustic-os-dev";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Production,
    Development,
}

impl Channel {
    const fn registry(self) -> &'static str {
        match self {
            Self::Production => REGISTRY_PROD,
            Self::Development => REGISTRY_DEV,
        }
    }

    const fn slug(self) -> &'static str {
        match self {
            Self::Production => "prod",
            Self::Development => "dev",
        }
    }
}

pub struct Installer {
    lang: Lang,
    channel: Channel,
    step: Step,
    tags: Vec<String>,
    selected_tag: Option<usize>,
    image_path: Option<PathBuf>,
    error: Option<String>,
    simulate: bool,
    auto: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    TagsLoaded(Result<Vec<String>, String>),
    TagSelected(usize),
    ChannelSelected(Channel),
    DownloadClicked,
    DownloadProgress(f32),
    DownloadFinished(Result<(), String>),
    DiskSelected(usize),
    DiskRefreshClicked,
    RpibootClicked,
    RpibootFinished(Result<(), String>),
    FlashClicked,
    FlashProgress(f32),
    FlashFinished(Result<(), String>),
}

enum Step {
    Loading,
    SelectRelease,
    Downloading { progress: f32 },
    SelectDisk {
        disks: Vec<Disk>,
        selected: Option<usize>,
    },
    RunningRpiboot,
    Flashing { progress: f32 },
    Done,
}

impl Installer {
    pub fn init(simulate: bool, auto: bool) -> (Self, Task<Message>) {
        let lang = Lang::detect();
        let channel = Channel::Production;
        let task = if simulate {
            simulate_tags()
        } else {
            load_tags(channel)
        };
        (
            Self {
                lang,
                channel,
                step: Step::Loading,
                tags: Vec::new(),
                selected_tag: None,
                image_path: None,
                error: None,
                simulate,
                auto,
            },
            task,
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TagsLoaded(Ok(tags)) => {
                self.tags = tags;
                self.step = Step::SelectRelease;
                if self.auto {
                    self.selected_tag = Some(0);
                    return delayed_message(Duration::from_secs(1), Message::DownloadClicked);
                }
                Task::none()
            }
            Message::TagsLoaded(Err(err))
            | Message::DownloadFinished(Err(err))
            | Message::FlashFinished(Err(err))
            | Message::RpibootFinished(Err(err)) => {
                self.handle_error(err);
                Task::none()
            }
            Message::TagSelected(index) => {
                self.selected_tag = Some(index);
                Task::none()
            }
            Message::ChannelSelected(channel) => self.select_channel(channel),
            Message::DownloadClicked => self.start_download(),
            Message::DownloadProgress(progress) => {
                if let Step::Downloading {
                    progress: current, ..
                } = &mut self.step
                {
                    *current = progress;
                }
                Task::none()
            }
            Message::DownloadFinished(Ok(()))
            | Message::RpibootFinished(Ok(())) => self.enter_disk_selection(),
            Message::DiskSelected(index) => {
                if let Step::SelectDisk { selected, .. } = &mut self.step {
                    *selected = Some(index);
                }
                if self.auto {
                    return delayed_message(Duration::from_secs(1), Message::FlashClicked);
                }
                Task::none()
            }
            Message::DiskRefreshClicked => {
                let old_selected = match &self.step {
                    Step::SelectDisk { selected, .. } => *selected,
                    _ => None,
                };
                let disks = get_disks(self.simulate);
                let selected = old_selected.filter(|&i| i < disks.len());
                let disk_count = disks.len();
                self.step = Step::SelectDisk { disks, selected };
                if self.auto {
                    if disk_count == 0 {
                        return delayed_message(
                            Duration::from_secs(1),
                            Message::DiskRefreshClicked,
                        );
                    }
                    return delayed_message(Duration::from_secs(1), Message::DiskSelected(0));
                }
                Task::none()
            }
            Message::RpibootClicked => self.start_rpiboot(),
            Message::FlashClicked => self.start_flash(),
            Message::FlashProgress(progress) => {
                if let Step::Flashing {
                    progress: current, ..
                } = &mut self.step
                {
                    *current = progress;
                }
                Task::none()
            }
            Message::FlashFinished(Ok(())) => {
                self.step = Step::Done;
                Task::none()
            }
        }
    }

    fn handle_error(&mut self, err: String) {
        self.error = Some(err);
        match &self.step {
            Step::Flashing { .. }
            | Step::SelectDisk { .. }
            | Step::RunningRpiboot => {
                let disks = get_disks(self.simulate);
                self.step = Step::SelectDisk {
                    disks,
                    selected: None,
                };
            }
            _ => {
                self.step = Step::SelectRelease;
            }
        }
    }

    fn select_channel(&mut self, channel: Channel) -> Task<Message> {
        if channel == self.channel {
            return Task::none();
        }
        self.channel = channel;
        self.selected_tag = None;
        self.tags.clear();
        self.error = None;
        self.image_path = None;
        self.step = Step::Loading;
        if self.simulate {
            simulate_tags()
        } else {
            load_tags(channel)
        }
    }

    fn enter_disk_selection(&mut self) -> Task<Message> {
        let disks = get_disks(self.simulate);
        let disk_count = disks.len();
        self.step = Step::SelectDisk {
            disks,
            selected: None,
        };
        if self.auto {
            return Task::perform(
                async move {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                },
                move |()| {
                    if disk_count > 0 {
                        Message::DiskSelected(0)
                    } else {
                        Message::DiskRefreshClicked
                    }
                },
            );
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let header = self.view_header();

        let body = container(self.view_body())
            .width(Fill)
            .height(Fill)
            .center_y(Fill);

        let footer = self.view_footer();

        let mut layout = column![header, body];

        if let Some(err) = &self.error {
            layout = layout.push(
                text(format!("{}: {err}", t(self.lang, Text::Error))).size(14),
            );
        }

        layout = layout.push(footer);
        layout = layout.width(Fill).max_width(560).height(Fill).spacing(16);

        container(layout)
            .width(Fill)
            .height(Fill)
            .center_x(Fill)
            .padding(40)
            .into()
    }

    fn view_header(&self) -> Element<'_, Message> {
        let title = text("Caustic OS Installer").size(22);

        if matches!(self.step, Step::Loading | Step::SelectRelease) {
            let channel_row = row![
                channel_button(self.channel, Channel::Production, self.lang),
                channel_button(self.channel, Channel::Development, self.lang),
            ]
            .spacing(8);

            column![title, channel_row].spacing(12).into()
        } else {
            title.into()
        }
    }

    fn view_body(&self) -> Element<'_, Message> {
        match &self.step {
            Step::Loading => {
                column![text(t(self.lang, Text::Loading)).size(16)].into()
            }
            Step::SelectRelease => self.view_releases(),
            Step::Downloading { progress } => column![
                text(t(self.lang, Text::Downloading)).size(18),
                progress_bar(0.0..=100.0, *progress),
                text(format!("{progress:.0}%")).size(14),
            ]
            .spacing(12)
            .into(),
            Step::SelectDisk { disks, selected } => self.view_disks(disks, *selected),
            Step::RunningRpiboot => {
                column![text(t(self.lang, Text::RpibootRunning)).size(18)].into()
            }
            Step::Flashing { progress } => column![
                text(t(self.lang, Text::Flashing)).size(18),
                progress_bar(0.0..=100.0, *progress),
                text(format!("{progress:.0}%")).size(14),
            ]
            .spacing(12)
            .into(),
            Step::Done => column![
                text(t(self.lang, Text::Done)).size(24),
                text(t(self.lang, Text::DoneHint)).size(16),
            ]
            .spacing(8)
            .into(),
        }
    }

    fn view_footer(&self) -> Element<'_, Message> {
        let footer: Option<Element<'_, Message>> = match &self.step {
            Step::SelectRelease if self.selected_tag.is_some() => Some(
                button(t(self.lang, Text::Download))
                    .width(Fill)
                    .style(button::primary)
                    .on_press(Message::DownloadClicked)
                    .into(),
            ),
            Step::SelectDisk { selected, .. } => {
                let mut actions = row![
                    button(t(self.lang, Text::Refresh))
                        .style(button::secondary)
                        .on_press(Message::DiskRefreshClicked),
                    Space::new().width(Fill),
                ]
                .spacing(8);

                if usbboot::is_available() || self.simulate {
                    actions = actions.push(
                        button(t(self.lang, Text::Rpiboot))
                            .style(button::secondary)
                            .on_press(Message::RpibootClicked),
                    );
                }

                let mut col = column![actions];

                if selected.is_some() {
                    col = col.push(
                        button(t(self.lang, Text::Flash))
                            .width(Fill)
                            .style(button::danger)
                            .on_press(Message::FlashClicked),
                    );
                }

                Some(col.spacing(8).into())
            }
            _ => None,
        };

        footer.unwrap_or_else(|| text("").into())
    }

    fn view_releases(&self) -> Element<'_, Message> {
        let mut list = column![];

        for (i, tag) in self.tags.iter().enumerate() {
            let is_selected = self.selected_tag == Some(i);
            let label = row![
                text(tag.as_str()).size(16),
                Space::new().width(Fill),
                if is_selected {
                    text("\u{2713}").size(16)
                } else {
                    text("")
                },
            ]
            .align_y(Center);

            list = list.push(
                button(label)
                    .width(Fill)
                    .style(if is_selected {
                        button::primary
                    } else {
                        button::secondary
                    })
                    .on_press(Message::TagSelected(i)),
            );
        }

        column![
            text(t(self.lang, Text::SelectRelease)).size(18),
            scrollable(list.spacing(4)).height(Fill),
        ]
        .spacing(12)
        .into()
    }

    fn view_disks(&self, disks: &[Disk], selected: Option<usize>) -> Element<'_, Message> {
        let warning = container(
            text(format!(
                "\u{26a0}\u{fe0f} {}",
                t(self.lang, Text::DataLossWarning)
            ))
            .size(14),
        )
        .padding(8);

        let mut list = column![];

        for (i, d) in disks.iter().enumerate() {
            let is_selected = selected == Some(i);

            let mut info_parts = vec![format!("{} GB", d.size_gb()), d.bus_type.clone()];
            if d.is_removable {
                info_parts.push(t(self.lang, Text::Removable).to_string());
            }

            let label = row![
                column![
                    text(d.description.clone()).size(16),
                    text(info_parts.join(" \u{00b7} ")).size(12),
                ],
                Space::new().width(Fill),
                if is_selected {
                    text("\u{2713}").size(18)
                } else {
                    text("")
                },
            ]
            .align_y(Center);

            list = list.push(
                button(label)
                    .width(Fill)
                    .style(if is_selected {
                        button::primary
                    } else {
                        button::secondary
                    })
                    .on_press(Message::DiskSelected(i)),
            );
        }

        column![
            warning,
            text(t(self.lang, Text::SelectDisk)).size(18),
            scrollable(list.spacing(4)).height(Fill),
        ]
        .spacing(12)
        .into()
    }

    fn start_download(&mut self) -> Task<Message> {
        let Some(index) = self.selected_tag else {
            return Task::none();
        };
        let Some(tag) = self.tags.get(index).cloned() else {
            return Task::none();
        };

        if self.simulate {
            self.step = Step::Downloading { progress: 0.0 };
            let straw = simulate_straw();
            return Task::sip(straw, Message::DownloadProgress, Message::DownloadFinished);
        }

        self.error = None;

        let registry = self.channel.registry().to_string();

        let Some(cache_dir) = cache_dir() else {
            self.error = Some("Could not determine cache directory".to_string());
            return Task::none();
        };

        let image_path = cache_dir
            .join(self.channel.slug())
            .join(format!("caustic-os-{tag}.img"));
        let partial_path = image_path.with_extension("img.partial");

        if let Some(parent) = image_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if image_path.exists()
            && std::fs::metadata(&image_path).is_ok_and(|m| m.len() > 0)
        {
            self.image_path = Some(image_path);
            let disks = get_disks(self.simulate);
            self.step = Step::SelectDisk {
                disks,
                selected: None,
            };
            return Task::none();
        }

        self.image_path = Some(image_path.clone());
        self.step = Step::Downloading { progress: 0.0 };

        let straw = run_with_progress(move |progress| async move {
            let manifest = caustic_oci::fetch_manifest(&registry, &tag).await?;
            let layer = caustic_oci::find_layer_by_suffix(&manifest, ".img")
                .ok_or(caustic_oci::Error::NoImageLayer)?;
            caustic_oci::pull_blob_streaming(&registry, &tag, layer, &partial_path, progress)
                .await?;
            tokio::fs::rename(&partial_path, &image_path)
                .await
                .map_err(|e| caustic_oci::Error::Io(e.to_string()))?;
            Ok::<(), caustic_oci::Error>(())
        });

        Task::sip(straw, Message::DownloadProgress, Message::DownloadFinished)
    }

    fn start_rpiboot(&mut self) -> Task<Message> {
        if self.simulate {
            self.step = Step::RunningRpiboot;
            return Task::perform(
                async {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    Ok::<(), String>(())
                },
                Message::RpibootFinished,
            );
        }

        if !usbboot::is_available() {
            self.error = Some(t(self.lang, Text::RpibootNotFound).to_string());
            return Task::none();
        }

        self.step = Step::RunningRpiboot;

        Task::perform(
            async { usbboot::run_rpiboot().await.map_err(|e| e.to_string()) },
            Message::RpibootFinished,
        )
    }

    fn start_flash(&mut self) -> Task<Message> {
        self.error = None;
        let Step::SelectDisk { disks, selected } = &self.step else {
            return Task::none();
        };
        let Some(index) = *selected else {
            return Task::none();
        };
        let Some(disk) = disks.get(index) else {
            return Task::none();
        };

        if self.simulate {
            self.step = Step::Flashing { progress: 0.0 };
            let straw = simulate_straw();
            return Task::sip(straw, Message::FlashProgress, Message::FlashFinished);
        }

        let Some(image) = self.image_path.clone() else {
            return Task::none();
        };

        let target = disk.device.clone();
        self.step = Step::Flashing { progress: 0.0 };

        let straw = run_with_progress(move |progress| flash::flash_image(image, target, progress));

        Task::sip(straw, Message::FlashProgress, Message::FlashFinished)
    }
}

fn delayed_message(delay: Duration, msg: Message) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(delay).await;
        },
        move |()| msg,
    )
}

fn channel_button(
    active: Channel,
    channel: Channel,
    lang: Lang,
) -> button::Button<'static, Message> {
    let label = match channel {
        Channel::Production => t(lang, Text::Production),
        Channel::Development => t(lang, Text::Development),
    };

    let is_active = active == channel;

    button(text(label).size(16))
        .width(Fill)
        .style(if is_active {
            button::primary
        } else {
            button::secondary
        })
        .on_press(Message::ChannelSelected(channel))
}

fn get_disks(simulate: bool) -> Vec<Disk> {
    if simulate {
        simulate_disks()
    } else {
        disk::list_disks()
    }
}

fn simulate_disks() -> Vec<Disk> {
    vec![
        Disk {
            device: "/dev/sda".into(),
            description: "SanDisk Ultra 64GB".into(),
            size: 64_000_000_000,
            is_removable: true,
            bus_type: "USB".into(),
        },
        Disk {
            device: "/dev/sdb".into(),
            description: "Samsung T7 500GB".into(),
            size: 500_107_862_016,
            is_removable: true,
            bus_type: "USB".into(),
        },
        Disk {
            device: "/dev/nvme0n1".into(),
            description: "WD Black SN750 1TB".into(),
            size: 1_000_000_000_000,
            is_removable: false,
            bus_type: "NVMe".into(),
        },
    ]
}

fn simulate_tags() -> Task<Message> {
    Task::perform(
        async {
            tokio::time::sleep(Duration::from_millis(500)).await;
            Ok::<_, String>(vec![
                "v1.0.0".to_string(),
                "v0.9.0".to_string(),
                "v0.8.0".to_string(),
            ])
        },
        Message::TagsLoaded,
    )
}

fn simulate_straw() -> impl Straw<(), f32, String> {
    sipper(async move |mut straw| {
        for i in 0..=100u8 {
            let () = straw.send(f32::from(i)).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok::<(), String>(())
    })
}

fn load_tags(channel: Channel) -> Task<Message> {
    let registry = channel.registry().to_string();
    Task::perform(
        async move {
            caustic_oci::list_tags(&registry)
                .await
                .map(|tags| {
                    tags.into_iter()
                        .filter(|tag| {
                            !(tag.starts_with("sha256-")
                                && std::path::Path::new(tag)
                                    .extension()
                                    .is_some_and(|ext| ext.eq_ignore_ascii_case("sig")))
                        })
                        .collect()
                })
                .map_err(|e| e.to_string())
        },
        Message::TagsLoaded,
    )
}

fn cache_dir() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("systems", "stargrid", "caustic")?;
    let cache = dirs.cache_dir().to_path_buf();
    std::fs::create_dir_all(&cache).ok()?;
    Some(cache)
}

fn run_with_progress<F, Fut, E>(f: F) -> impl Straw<(), f32, String>
where
    F: FnOnce(Arc<dyn Fn(u64, u64) + Send + Sync>) -> Fut + Send,
    Fut: std::future::Future<Output = Result<(), E>> + Send,
    E: std::fmt::Display + Send + 'static,
{
    sipper(async move |mut straw| {
        let (tx, mut rx) = tokio::sync::watch::channel(0.0f32);

        let progress: Arc<dyn Fn(u64, u64) + Send + Sync> = Arc::new(move |done, total| {
            let pct = done
                .min(total)
                .saturating_mul(100)
                .checked_div(total)
                .map_or(0.0, |ratio| f32::from(u8::try_from(ratio).unwrap_or(100)));
            let _ = tx.send(pct);
        });

        let mut fut = std::pin::pin!(f(progress));

        loop {
            tokio::select! {
                result = &mut fut => {
                    return result.map_err(|e| e.to_string());
                }
                Ok(()) = rx.changed() => {
                    let pct = *rx.borrow();
                    let () = straw.send(pct).await;
                }
            }
        }
    })
}
