use std::path::PathBuf;
use std::sync::Arc;

use iced::task::{sipper, Straw};
use iced::widget::{
    button, column, container, progress_bar, row, scrollable, text, Space,
};
use iced::{Center, Element, Fill, Task};

use crate::disk::{self, Disk};
use crate::flash;
use crate::i18n::{t, Lang, Text};

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
}

pub struct Installer {
    lang: Lang,
    channel: Channel,
    step: Step,
    tags: Vec<String>,
    selected_tag: Option<usize>,
    error: Option<String>,
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
    FlashClicked,
    FlashProgress(f32),
    FlashFinished(Result<(), String>),
    LanguageSelected(Lang),
}

enum Step {
    Loading,
    SelectRelease,
    Downloading {
        progress: f32,
        image_path: PathBuf,
    },
    SelectDisk {
        image_path: PathBuf,
        disks: Vec<Disk>,
        selected: Option<usize>,
    },
    Flashing {
        progress: f32,
    },
    Done,
}

impl Installer {
    pub fn init() -> (Self, Task<Message>) {
        let lang = Lang::detect();
        let channel = Channel::Production;
        let task = load_tags(channel);
        (
            Self {
                lang,
                channel,
                step: Step::Loading,
                tags: Vec::new(),
                selected_tag: None,
                error: None,
            },
            task,
        )
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::TagsLoaded(Ok(tags)) => {
                self.tags = tags;
                self.step = Step::SelectRelease;
                Task::none()
            }
            Message::TagsLoaded(Err(err))
            | Message::DownloadFinished(Err(err))
            | Message::FlashFinished(Err(err)) => {
                self.error = Some(err);
                self.step = Step::SelectRelease;
                Task::none()
            }
            Message::TagSelected(index) => {
                self.selected_tag = Some(index);
                Task::none()
            }
            Message::ChannelSelected(channel) => {
                if channel == self.channel {
                    Task::none()
                } else {
                    self.channel = channel;
                    self.selected_tag = None;
                    self.tags.clear();
                    self.error = None;
                    self.step = Step::Loading;
                    load_tags(channel)
                }
            }
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
            Message::DownloadFinished(Ok(())) => {
                if let Step::Downloading { image_path, .. } = &self.step {
                    let disks = disk::list_disks();
                    self.step = Step::SelectDisk {
                        image_path: image_path.clone(),
                        disks,
                        selected: None,
                    };
                }
                Task::none()
            }
            Message::DiskSelected(index) => {
                if let Step::SelectDisk { selected, .. } = &mut self.step {
                    *selected = Some(index);
                }
                Task::none()
            }
            Message::FlashClicked => self.start_flash(),
            Message::FlashProgress(progress) => {
                if let Step::Flashing {
                    progress: current,
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
            Message::LanguageSelected(lang) => {
                self.lang = lang;
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let header = row![
            text("Caustic OS Installer").size(22),
            Space::new().width(Fill),
            button(text(match self.lang {
                Lang::En => "DE",
                Lang::De => "EN",
            }))
            .on_press(Message::LanguageSelected(match self.lang {
                Lang::En => Lang::De,
                Lang::De => Lang::En,
            })),
        ]
        .align_y(Center);

        let channel_row = row![
            channel_button(self.channel, Channel::Production, self.lang),
            channel_button(self.channel, Channel::Development, self.lang),
        ]
        .spacing(8);

        let content: Element<'_, Message> = match &self.step {
            Step::Loading => {
                column![text(t(self.lang, Text::Loading)).size(16)].into()
            }
            Step::SelectRelease => self.view_releases(),
            Step::Downloading { progress, .. } => {
                column![
                    text(t(self.lang, Text::Downloading)).size(18),
                    progress_bar(0.0..=100.0, *progress),
                    text(format!("{progress:.0}%")).size(14),
                ]
                .spacing(12)
                .into()
            }
            Step::SelectDisk {
                disks, selected, ..
            } => self.view_disks(disks, selected.as_ref()),
            Step::Flashing { progress } => {
                column![
                    text(t(self.lang, Text::Flashing)).size(18),
                    progress_bar(0.0..=100.0, *progress),
                    text(format!("{progress:.0}%")).size(14),
                ]
                .spacing(12)
                .into()
            }
            Step::Done => {
                column![
                    text(t(self.lang, Text::Done)).size(24),
                    text(t(self.lang, Text::DoneHint)).size(16),
                ]
                .spacing(8)
                .into()
            }
        };

        let mut layout = column![header].spacing(20);

        if matches!(self.step, Step::Loading | Step::SelectRelease) {
            layout = layout.push(channel_row);
        }

        layout = layout.push(content);

        if let Some(err) = &self.error {
            layout = layout.push(
                text(format!("{}: {err}", t(self.lang, Text::Error))).size(14),
            );
        }

        container(layout.spacing(16))
            .padding(40)
            .max_width(560)
            .center(Fill)
            .into()
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

        let mut col = column![
            text(t(self.lang, Text::SelectRelease)).size(18),
            scrollable(list.spacing(4)).height(280),
        ]
        .spacing(12);

        if self.selected_tag.is_some() {
            col = col.push(
                button(t(self.lang, Text::Download))
                    .style(button::primary)
                    .on_press(Message::DownloadClicked),
            );
        }

        col.into()
    }

    fn view_disks(&self, disks: &[Disk], selected: Option<&usize>) -> Element<'_, Message> {        let warning = container(text(format!(
            "\u{26a0}\u{fe0f} {}",
            t(self.lang, Text::DataLossWarning)
        )))
        .padding(8);

        let mut list = column![];

        for (i, d) in disks.iter().enumerate() {
            let is_selected = selected == Some(&i);
            let label = row![
                column![
                    text(d.name.clone()).size(16),
                    text(format!("{} GB", d.size_gb)).size(12),
                ],
                Space::new().width(Fill),
                if is_selected {
                    text("\u{2713}")
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

        let mut col = column![
            warning,
            text(t(self.lang, Text::SelectDisk)).size(18),
            scrollable(list.spacing(4)).height(200),
        ]
        .spacing(12);

        if selected.is_some() {
            col = col.push(
                button(t(self.lang, Text::Flash))
                    .style(button::danger)
                    .on_press(Message::FlashClicked),
            );
        }

        col.into()
    }

    fn start_download(&mut self) -> Task<Message> {
        let Some(index) = self.selected_tag else {
            return Task::none();
        };
        let Some(tag) = self.tags.get(index).cloned() else {
            return Task::none();
        };

        let registry = self.channel.registry().to_string();
        let image_path = std::env::temp_dir().join(format!("caustic-os-{tag}.img"));

        self.step = Step::Downloading {
            progress: 0.0,
            image_path: image_path.clone(),
        };

        let straw = run_with_progress(move |progress| async move {
            let manifest = caustic_oci::fetch_manifest(&registry, &tag).await?;
            let layer = caustic_oci::find_layer_by_suffix(&manifest, ".img")
                .ok_or(caustic_oci::Error::NoImageLayer)?;
            caustic_oci::pull_blob_streaming(&registry, &tag, layer, &image_path, progress).await
        });

        Task::sip(straw, Message::DownloadProgress, Message::DownloadFinished)
    }

    fn start_flash(&mut self) -> Task<Message> {
        let Step::SelectDisk {
            image_path,
            disks,
            selected,
        } = &self.step
        else {
            return Task::none();
        };

        let Some(index) = selected else {
            return Task::none();
        };

        let Some(disk) = disks.get(*index) else {
            return Task::none();
        };

        let image = image_path.clone();
        let target = disk.path.clone();

        self.step = Step::Flashing { progress: 0.0 };

        let straw = run_with_progress(move |progress| flash::flash_image(image, target, progress));

        Task::sip(straw, Message::FlashProgress, Message::FlashFinished)
    }
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

fn run_with_progress<F, Fut, E>(f: F) -> impl Straw<(), f32, String>
where
    F: FnOnce(Arc<dyn Fn(u64, u64) + Send + Sync>) -> Fut + Send,
    Fut: std::future::Future<Output = Result<(), E>> + Send,
    E: std::fmt::Display + Send + 'static,
{
    sipper(async move |mut straw| {
        let (tx, mut rx) = tokio::sync::watch::channel(0.0f32);

        let progress: Arc<dyn Fn(u64, u64) + Send + Sync> = Arc::new(move |done, total| {
            let pct = (done.min(total) * 100)
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
