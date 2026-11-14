use genie_formats::http::Url;
use iced::widget::{column, container, operation, pin, text, text_input};
use iced::{Element, Task};

use crate::chrome::{self, Controls};
use crate::location::Location;
use crate::{font, style};

use super::{Address, AddressField, Genie, Message};

const FIELD: &str = "address";
const WIDTH: f32 = 480.0;

impl Genie {
    /// Asks for the address of a file, or of a track hub when `hub` is true.
    pub(super) fn ask_address(&mut self, hub: bool) -> Task<Message> {
        self.close_menus();
        self.address = Some(AddressField {
            hub,
            ..AddressField::default()
        });
        Task::batch([operation::focus(FIELD), operation::select_all(FIELD)])
    }

    pub(super) fn address(&mut self, action: Address) -> Task<Message> {
        let Some(field) = &mut self.address else {
            return Task::none();
        };
        match action {
            Address::Edit(text) => {
                field.text = text;
                field.error = None;
            }
            Address::Submit => match Url::parse(&field.text) {
                Ok(url) => {
                    let hub = field.hub;
                    self.address = None;
                    return if hub {
                        self.open_hub(Location::Url(url), true)
                    } else {
                        self.open(Location::Url(url))
                    };
                }
                Err(error) => field.error = Some(error.to_string()),
            },
        }
        Task::none()
    }

    pub(super) fn address_view(&self) -> Option<Element<'_, Message>> {
        let field = self.address.as_ref()?;
        let (example, words) = if field.hub {
            (
                "https://example.org/hub.txt",
                "Open a track hub from the web",
            )
        } else {
            (
                "https://example.org/data/genome.2bit",
                "Open a file from the web",
            )
        };
        let input = text_input(example, &field.text)
            .id(FIELD)
            .font(font::MONO)
            .size(12)
            .padding([3, 8])
            .style(style::locus)
            .on_input(|text| Message::Address(Address::Edit(text)))
            .on_submit(Message::Address(Address::Submit));
        let caption = text(words).size(11).style(style::muted);
        let error = field.error.as_deref().map(|error| text(error).size(11));
        let card = container(column![caption, input].push(error).spacing(6))
            .width(WIDTH)
            .padding(8)
            .style(style::menu);
        Some(
            pin(card)
                .x(chrome::locus_x(Controls::CURRENT))
                .y(chrome::BAR_HEIGHT + 2.0)
                .into(),
        )
    }
}
