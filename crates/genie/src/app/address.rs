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
    pub(super) fn ask_address(&mut self) -> Task<Message> {
        self.close_menus();
        self.address = Some(AddressField::default());
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
                    self.address = None;
                    return self.open(Location::Url(url));
                }
                Err(error) => field.error = Some(error.to_string()),
            },
        }
        Task::none()
    }

    pub(super) fn address_view(&self) -> Option<Element<'_, Message>> {
        let field = self.address.as_ref()?;
        let input = text_input("https://example.org/data/genome.2bit", &field.text)
            .id(FIELD)
            .font(font::MONO)
            .size(12)
            .padding([3, 8])
            .style(style::locus)
            .on_input(|text| Message::Address(Address::Edit(text)))
            .on_submit(Message::Address(Address::Submit));
        let caption = text("Open a file from the web")
            .size(11)
            .style(style::muted);
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
