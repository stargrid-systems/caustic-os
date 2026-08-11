use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use iced::task::{Straw, sipper};
use iced::widget::{Space, button, column, container, progress_bar, row, scrollable, text};
use iced::{Center, Element, Fill, Task};

use crate::disk::{self, Disk};
use crate::i18n::{Lang, Text, t};
use crate::widgets::pager;
use crate::{flash, usbboot};

const REGISTRY_PROD: &str = "ghcr.io/stargrid-systems/caustic-os";
const REGISTRY_DEV: &str = "ghcr.io/stargrid-systems/caustic-os-dev";
const PAGE_SIZE: usize = 10;

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
    tag_metas: HashMap<String, TagMeta>,
    current_page: usize,
    selected_tag: Option<usize>,
    image_path: Option<PathBuf>,
    error: Option<String>,
    simulate: bool,
    auto: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum FetchStatus {
    #[default]
    Idle,
    Loading,
    Done,
    Failed,
}

#[derive(Clone, Debug, Default)]
struct TagMeta {
    created: Option<String>,
    status: FetchStatus,
}

#[derive(Debug, Clone)]
pub enum Message {
    TagsLoaded(Result<Vec<String>, String>),
    TagSelected(usize),
    TagMetaLoaded {
        tag: String,
        created: Result<Option<String>, String>,
    },
    NextPage,
    PrevPage,
    ChannelSelected(Channel),
    DownloadClicked,
    LocalFileClicked,
    LocalFilePicked(Option<PathBuf>),
    DownloadProgress(f32),
    DownloadFinished(Result<(), String>),
    VerifyResult(Result<(), String>),
    VerifyContinue,
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
    Downloading {
        progress: f32,
    },
    Verify,
    VerifyDisabled,
    SelectDisk {
        disks: Vec<Disk>,
        selected: Option<usize>,
    },
    RunningRpiboot,
    Flashing {
        progress: f32,
    },
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
                tag_metas: HashMap::new(),
                current_page: 0,
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
                self.tag_metas.clear();
                self.current_page = 0;
                self.step = Step::SelectRelease;
                if self.auto {
                    self.selected_tag = Some(0);
                    return delayed_message(Duration::from_secs(1), Message::DownloadClicked);
                }
                self.fetch_visible_page()
            }
            Message::TagsLoaded(Err(err))
            | Message::DownloadFinished(Err(err))
            | Message::VerifyResult(Err(err))
            | Message::FlashFinished(Err(err))
            | Message::RpibootFinished(Err(err)) => {
                self.handle_error(err);
                Task::none()
            }
            Message::TagSelected(index) => {
                self.selected_tag = Some(index);
                Task::none()
            }
            Message::TagMetaLoaded { tag, created } => {
                self.store_tag_meta(&tag, created);
                Task::none()
            }
            Message::NextPage => self.goto_page(self.current_page + 1),
            Message::PrevPage => self.goto_page(self.current_page.saturating_sub(1)),
            Message::ChannelSelected(channel) => self.select_channel(channel),
            Message::DownloadClicked => self.start_download(),
            Message::LocalFileClicked => pick_image_file(),
            Message::LocalFilePicked(maybe_path) => self.use_local_file(maybe_path),
            Message::DownloadProgress(progress) => {
                if let Step::Downloading {
                    progress: current, ..
                } = &mut self.step
                {
                    *current = progress;
                }
                Task::none()
            }
            Message::DownloadFinished(Ok(())) => self.after_download(),
            Message::VerifyResult(Ok(()))
            | Message::VerifyContinue
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
            Step::Flashing { .. } | Step::SelectDisk { .. } | Step::RunningRpiboot => {
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
        self.tag_metas.clear();
        self.current_page = 0;
        self.error = None;
        self.image_path = None;
        self.step = Step::Loading;
        if self.simulate {
            simulate_tags()
        } else {
            load_tags(channel)
        }
    }

    fn use_local_file(&mut self, maybe_path: Option<PathBuf>) -> Task<Message> {
        let Some(path) = maybe_path else {
            return Task::none();
        };
        if validate_image_file(&path).is_err() {
            self.handle_error(t(self.lang, Text::LocalFileInvalid).to_string());
            return Task::none();
        }
        self.error = None;
        self.image_path = Some(path);
        self.enter_disk_selection()
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
            layout = layout.push(text(format!("{}: {err}", t(self.lang, Text::Error))).size(14));
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
            Step::Loading => column![text(t(self.lang, Text::Loading)).size(16)].into(),
            Step::SelectRelease => self.view_releases(),
            Step::Downloading { progress } => column![
                text(t(self.lang, Text::Downloading)).size(18),
                progress_bar(0.0..=100.0, *progress),
                text(format!("{progress:.0}%")).size(14),
            ]
            .spacing(12)
            .into(),
            Step::Verify => column![text(t(self.lang, Text::Verifying)).size(18)].into(),
            Step::VerifyDisabled => column![
                text(format!("⚠️ {}", t(self.lang, Text::VerifyDisabled))).size(18),
                text(t(self.lang, Text::VerifyDisabledHint)).size(14),
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
            Step::SelectRelease => {
                let mut col = column![
                    button(t(self.lang, Text::UseLocalFile))
                        .width(Fill)
                        .style(button::secondary)
                        .on_press(Message::LocalFileClicked)
                ];

                if self.selected_tag.is_some() {
                    col = col.push(
                        button(t(self.lang, Text::Download))
                            .width(Fill)
                            .style(button::primary)
                            .on_press(Message::DownloadClicked),
                    );
                }

                Some(col.spacing(8).into())
            }
            Step::VerifyDisabled => Some(
                button(t(self.lang, Text::Continue))
                    .width(Fill)
                    .style(button::primary)
                    .on_press(Message::VerifyContinue)
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
        let total_pages = self.total_pages();
        let start = self.current_page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(self.tags.len());

        let mut list = column![];

        for i in start..end {
            let tag = &self.tags[i];
            let is_selected = self.selected_tag == Some(i);

            let date_text = self.tag_metas.get(tag).and_then(|meta| match meta.status {
                FetchStatus::Loading => Some(t(self.lang, Text::LoadingShort).to_string()),
                FetchStatus::Done => meta.created.as_deref().map(format_created),
                FetchStatus::Idle | FetchStatus::Failed => None,
            });

            let mut info = column![text(tag.as_str()).size(16)];
            if let Some(d) = date_text {
                info = info.push(text(d).size(12));
            }

            let label = row![
                info,
                Space::new().width(Fill),
                if is_selected {
                    text("✓").size(16)
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

        let pager = pager(
            self.current_page,
            total_pages,
            Some(Message::PrevPage),
            Some(Message::NextPage),
            t(self.lang, Text::Previous),
            t(self.lang, Text::Next),
        );

        column![
            text(t(self.lang, Text::SelectRelease)).size(18),
            scrollable(list.spacing(4)).height(Fill),
            pager,
        ]
        .spacing(12)
        .into()
    }

    fn view_disks(&self, disks: &[Disk], selected: Option<usize>) -> Element<'_, Message> {
        let warning =
            container(text(format!("⚠️ {}", t(self.lang, Text::DataLossWarning))).size(14))
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
                    text(info_parts.join(" · ")).size(12),
                ],
                Space::new().width(Fill),
                if is_selected {
                    text("✓").size(18)
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

        if image_path.exists() && std::fs::metadata(&image_path).is_ok_and(|m| m.len() > 0) {
            self.image_path = Some(image_path);
            return self.after_download();
        }

        self.image_path = Some(image_path.clone());
        self.step = Step::Downloading { progress: 0.0 };

        let straw = run_with_progress(move |progress| async move {
            let manifest = caustic_oci::fetch_manifest(&registry, &tag, None).await?;
            let layer = caustic_oci::find_layer_by_suffix(&manifest, ".img")
                .ok_or(caustic_oci::Error::NoImageLayer)?;
            caustic_oci::pull_blob_streaming(&registry, &tag, layer, &partial_path, progress, None)
                .await?;
            tokio::fs::rename(&partial_path, &image_path)
                .await
                .map_err(|e| caustic_oci::Error::Io(e.to_string()))?;
            Ok::<(), caustic_oci::Error>(())
        });

        Task::sip(straw, Message::DownloadProgress, Message::DownloadFinished)
    }

    fn after_download(&mut self) -> Task<Message> {
        if self.simulate || caustic_oci_key::COSIGN_PUB.is_empty() {
            tracing::warn!("signature verification is disabled (development build)");
            self.step = Step::VerifyDisabled;
            if self.auto {
                return delayed_message(Duration::from_secs(1), Message::VerifyContinue);
            }
            return Task::none();
        }
        self.step = Step::Verify;
        self.start_verify()
    }

    fn start_verify(&self) -> Task<Message> {
        let Some(index) = self.selected_tag else {
            return Task::none();
        };
        let Some(tag) = self.tags.get(index).cloned() else {
            return Task::none();
        };

        let registry = self.channel.registry().to_string();
        let public_key = caustic_oci_key::COSIGN_PUB.to_vec();

        Task::perform(
            async move {
                let digest = caustic_oci::resolve_digest(&registry, &tag, None).await?;
                caustic_oci::verify_artifact(&registry, &digest, &public_key, None).await
            },
            |result: Result<(), caustic_oci::Error>| match result {
                Ok(()) => Message::VerifyResult(Ok(())),
                Err(err) => Message::VerifyResult(Err(err.to_string())),
            },
        )
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

    fn store_tag_meta(&mut self, tag: &str, created: Result<Option<String>, String>) {
        if let Some(meta) = self.tag_metas.get_mut(tag) {
            match created {
                Ok(created) => {
                    meta.created = created;
                    meta.status = FetchStatus::Done;
                }
                Err(_) => meta.status = FetchStatus::Failed,
            }
        }
    }

    fn goto_page(&mut self, page: usize) -> Task<Message> {
        let target = page.min(self.total_pages().saturating_sub(1));
        if target != self.current_page {
            self.current_page = target;
            return self.fetch_visible_page();
        }
        Task::none()
    }

    const fn total_pages(&self) -> usize {
        if self.tags.is_empty() {
            1
        } else {
            self.tags.len().div_ceil(PAGE_SIZE)
        }
    }

    fn fetch_visible_page(&mut self) -> Task<Message> {
        if self.simulate {
            return Task::none();
        }

        let start = self.current_page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(self.tags.len());
        let registry = self.channel.registry().to_string();

        let mut tasks = Vec::new();
        for i in start..end {
            let tag = self.tags[i].clone();
            let entry = self.tag_metas.entry(tag.clone()).or_default();
            if !should_fetch(entry.status) {
                continue;
            }
            entry.status = FetchStatus::Loading;

            let registry = registry.clone();
            let tag_for_msg = tag.clone();
            tasks.push(Task::perform(
                async move {
                    let manifest = caustic_oci::fetch_manifest(&registry, &tag, None).await?;
                    Ok::<_, caustic_oci::Error>(caustic_oci::extract_created(&manifest))
                },
                move |created: Result<Option<String>, caustic_oci::Error>| Message::TagMetaLoaded {
                    tag: tag_for_msg,
                    created: created.map_err(|e| e.to_string()),
                },
            ));
        }

        Task::batch(tasks)
    }
}

fn pick_image_file() -> Task<Message> {
    Task::perform(
        async {
            rfd::AsyncFileDialog::new()
                .add_filter("images", &["img", "iso", "gz"])
                .pick_file()
                .await
                .map(|handle| handle.path().to_path_buf())
        },
        Message::LocalFilePicked,
    )
}

fn validate_image_file(path: &Path) -> Result<(), ()> {
    let valid_ext = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "img" | "iso" | "gz"));
    if !valid_ext {
        return Err(());
    }
    let len = std::fs::metadata(path).map(|m| m.len()).map_err(|_| ())?;
    if len == 0 {
        return Err(());
    }
    Ok(())
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
                "2026.08.11".to_string(),
                "2026.08.04".to_string(),
                "2026.07.28".to_string(),
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

fn is_release_tag(tag: &str) -> bool {
    fn numeric(p: &str) -> bool {
        !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())
    }
    let mut parts = tag.split('.');
    let Some(y) = parts.next() else {
        return false;
    };
    let Some(m) = parts.next() else {
        return false;
    };
    let Some(p) = parts.next() else {
        return false;
    };
    numeric(y) && numeric(m) && numeric(p) && parts.next().is_none()
}

fn release_sort_key(tag: &str) -> (u32, u32, u32) {
    let mut parts = tag.split('.');
    let y = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let m = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let p = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    (y, m, p)
}

fn load_tags(channel: Channel) -> Task<Message> {
    let registry = channel.registry().to_string();
    Task::perform(
        async move {
            caustic_oci::list_tags(&registry, None)
                .await
                .map(|tags| {
                    let mut releases: Vec<String> =
                        tags.into_iter().filter(|tag| is_release_tag(tag)).collect();
                    releases.sort_by_key(|a| std::cmp::Reverse(release_sort_key(a)));
                    releases
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

const fn should_fetch(status: FetchStatus) -> bool {
    matches!(status, FetchStatus::Idle | FetchStatus::Failed)
}

fn format_created(raw: &str) -> String {
    let b = raw.as_bytes();
    let valid = b.len() >= 10
        && b[0..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit);
    if valid {
        raw[..10].to_string()
    } else {
        raw.to_string()
    }
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

#[cfg(test)]
mod release_date_tests {
    use super::*;

    fn installer_with_tags(n: usize) -> Installer {
        let (mut inst, _task) = Installer::init(true, false);
        inst.tags = (0..n).map(|i| format!("v{i}")).collect();
        inst.current_page = 0;
        inst
    }

    #[test]
    fn total_pages_empty_is_one() {
        let inst = installer_with_tags(0);
        assert_eq!(inst.total_pages(), 1);
    }

    #[test]
    fn total_pages_exact_multiple() {
        let inst = installer_with_tags(20);
        assert_eq!(inst.total_pages(), 2);
    }

    #[test]
    fn total_pages_partial_page() {
        let inst = installer_with_tags(23);
        assert_eq!(inst.total_pages(), 3);
    }

    #[test]
    fn goto_page_clamps_high() {
        let mut inst = installer_with_tags(15);
        let _ = inst.goto_page(100);
        assert_eq!(inst.current_page, 1);
    }

    #[test]
    fn goto_page_clamps_to_last() {
        let mut inst = installer_with_tags(25);
        let _ = inst.goto_page(usize::MAX);
        assert_eq!(inst.current_page, 2);
    }

    #[test]
    fn goto_page_no_op_when_same() {
        let mut inst = installer_with_tags(25);
        inst.current_page = 1;
        let _ = inst.goto_page(1);
        assert_eq!(inst.current_page, 1);
    }

    #[test]
    fn should_fetch_retries_failed_and_idle() {
        assert!(should_fetch(FetchStatus::Idle));
        assert!(should_fetch(FetchStatus::Failed));
        assert!(!should_fetch(FetchStatus::Loading));
        assert!(!should_fetch(FetchStatus::Done));
    }

    #[test]
    fn format_created_strips_time_portion() {
        assert_eq!(format_created("2024-01-02T03:04:05Z"), "2024-01-02");
    }

    #[test]
    fn format_created_returns_date_only() {
        assert_eq!(format_created("2026-08-11"), "2026-08-11");
    }

    #[test]
    fn format_created_keeps_non_date_unchanged() {
        assert_eq!(format_created("not-a-date"), "not-a-date");
        assert_eq!(format_created("v1.0.0"), "v1.0.0");
    }

    #[test]
    fn format_created_short_input_unchanged() {
        assert_eq!(format_created("2024"), "2024");
    }
}

#[cfg(test)]
mod tests {
    use super::{is_release_tag, release_sort_key};

    #[test]
    fn accepts_calver_tags() {
        assert!(is_release_tag("2026.08.11"));
        assert!(is_release_tag("2026.8.11"));
        assert!(is_release_tag("2026.12.0"));
        assert!(is_release_tag("1999.1.100"));
        assert!(is_release_tag("0000.00.00"));
    }

    #[test]
    fn rejects_non_release_tags() {
        assert!(!is_release_tag("sha256-deadbeef.sig"));
        assert!(!is_release_tag("sha256-deadbeef"));
        assert!(!is_release_tag("main"));
        assert!(!is_release_tag("latest"));
        assert!(!is_release_tag("v1.0.0"));
        assert!(!is_release_tag("abc1234"));
        assert!(!is_release_tag("2026.08"));
        assert!(!is_release_tag("2026.08.11.1"));
        assert!(!is_release_tag("2026.08.11-rc1"));
        assert!(!is_release_tag(""));
        assert!(!is_release_tag("2026.08.11 "));
    }

    #[test]
    fn sort_key_is_leading_zero_invariant() {
        assert_eq!(release_sort_key("2026.08.11"), (2026, 8, 11));
        assert_eq!(
            release_sort_key("2026.08.11"),
            release_sort_key("2026.8.11")
        );
    }

    #[test]
    fn sort_key_orders_newest_first() {
        let mut tags = vec![
            "2026.07.28".to_string(),
            "2026.08.11".to_string(),
            "2026.08.04".to_string(),
            "2025.12.31".to_string(),
        ];
        tags.sort_by_key(|a| std::cmp::Reverse(release_sort_key(a)));
        assert_eq!(
            tags,
            vec!["2026.08.11", "2026.08.04", "2026.07.28", "2025.12.31"]
        );
    }
}
