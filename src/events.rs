use esp_idf_svc::eventloop::{
    EspEvent, EspEventDeserializer, EspEventPostData, EspEventSerializer, EspEventSource,
};
use esp_inputs::Event as InputEvent;
use std::ffi::CStr;

#[derive(Clone, Copy)]
pub enum Event {
    DisplayFrame,
    Input(InputEvent),
}

unsafe impl EspEventSource for Event {
    fn source() -> Option<&'static CStr> {
        Some(CStr::from_bytes_with_nul(b"Event\0").unwrap())
    }
}

impl EspEventSerializer for Event {
    type Data<'a> = Self;

    fn serialize<F, R>(event: &Self::Data<'_>, f: F) -> R
    where
        F: FnOnce(&EspEventPostData) -> R,
    {
        f(&unsafe { EspEventPostData::new(Self::source().unwrap(), Self::event_id(), event) })
    }
}

impl EspEventDeserializer for Event {
    type Data<'a> = Self;

    fn deserialize<'a>(data: &EspEvent<'a>) -> Self::Data<'a> {
        *unsafe { data.as_payload::<Self>() }
    }
}

impl From<InputEvent> for Event {
    fn from(event: InputEvent) -> Self {
        Self::Input(event)
    }
}
