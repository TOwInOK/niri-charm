use std::collections::HashMap;
use std::path::PathBuf;

use niri_ipc::PickedColor;
use zbus::zvariant::OwnedValue;
use zbus::{fdo, interface, zvariant};

use super::{request_name, Start};

pub struct Screenshot {
    to_niri: calloop::channel::Sender<ScreenshotToNiri>,
}

pub enum ScreenshotToNiri {
    TakeScreenshot {
        include_cursor: bool,
        reply: async_channel::Sender<anyhow::Result<PathBuf>>,
    },
    PickColor(async_channel::Sender<Option<PickedColor>>),
}

#[interface(name = "org.gnome.Shell.Screenshot")]
impl Screenshot {
    async fn screenshot(
        &self,
        include_cursor: bool,
        _flash: bool,
        _filename: PathBuf,
    ) -> fdo::Result<(bool, PathBuf)> {
        let (tx, rx) = async_channel::bounded(1);
        if let Err(err) = self.to_niri.send(ScreenshotToNiri::TakeScreenshot {
            include_cursor,
            reply: tx,
        }) {
            warn!("error sending message to niri: {err:?}");
            return Err(fdo::Error::Failed("internal error".to_owned()));
        }

        let filename = match rx.recv().await {
            Ok(Ok(filename)) => filename,
            Ok(Err(err)) => {
                return Err(fdo::Error::Failed(format!(
                    "error taking a screenshot: {err:#}"
                )));
            }
            Err(err) => {
                warn!("error receiving message from niri: {err:?}");
                return Err(fdo::Error::Failed("internal error".to_owned()));
            }
        };

        Ok((true, filename))
    }

    async fn pick_color(&self) -> fdo::Result<HashMap<String, OwnedValue>> {
        let (tx, rx) = async_channel::bounded(1);
        if let Err(err) = self.to_niri.send(ScreenshotToNiri::PickColor(tx)) {
            warn!("error sending pick color message to niri: {err:?}");
            return Err(fdo::Error::Failed("internal error".to_owned()));
        }

        let color = match rx.recv().await {
            Ok(Some(color)) => color,
            Ok(None) => {
                return Err(fdo::Error::Failed("no color picked".to_owned()));
            }
            Err(err) => {
                warn!("error receiving message from niri: {err:?}");
                return Err(fdo::Error::Failed("internal error".to_owned()));
            }
        };

        let mut result = HashMap::new();
        let [r, g, b] = color.rgb;
        result.insert(
            "color".to_string(),
            zvariant::OwnedValue::try_from(zvariant::Value::from((r, g, b))).unwrap(),
        );

        Ok(result)
    }
}

impl Screenshot {
    pub fn new(to_niri: calloop::channel::Sender<ScreenshotToNiri>) -> Self {
        Self { to_niri }
    }
}

impl Start for Screenshot {
    fn start(self, monitor: bool) -> anyhow::Result<zbus::blocking::Connection> {
        let conn = zbus::blocking::Connection::session()?;
        conn.object_server()
            .at("/org/gnome/Shell/Screenshot", self)?;
        request_name(&conn, "org.gnome.Shell.Screenshot", monitor)?;

        Ok(conn)
    }
}
