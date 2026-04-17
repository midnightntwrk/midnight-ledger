use crate::conversions::event_details_to_value;
use crate::conversions::event_source_to_value;
use js_sys::Uint8Array;
use ledger::events::Event as LedgerEvent;
use onchain_runtime_wasm::from_value_ser;
use serialize::tagged_serialize;
use storage::db::InMemoryDB;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Debug)]
pub struct Event(pub(crate) LedgerEvent<InMemoryDB>);

impl From<LedgerEvent<InMemoryDB>> for Event {
    fn from(inner: LedgerEvent<InMemoryDB>) -> Event {
        Event(inner)
    }
}

#[wasm_bindgen]
impl Event {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<Event, JsError> {
        Err(JsError::new(
            "Event cannot be constructed directly through the WASM API.",
        ))
    }

    pub fn serialize(&self) -> Result<Uint8Array, JsError> {
        let mut res = Vec::new();
        tagged_serialize(&self.0, &mut res)?;
        Ok(Uint8Array::from(&res[..]))
    }

    pub fn deserialize(raw: Uint8Array) -> Result<Event, JsError> {
        Ok(Event(from_value_ser(raw, "Event")?))
    }

    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self, compact: Option<bool>) -> String {
        if compact.unwrap_or(false) {
            format!("{:?}", &self.0)
        } else {
            format!("{:#?}", &self.0)
        }
    }

    #[wasm_bindgen(getter = "source")]
    pub fn source(&self) -> Result<JsValue, JsError> {
        event_source_to_value(&self.0.source)
    }

    #[wasm_bindgen(getter = "content")]
    pub fn content(&self) -> Result<JsValue, JsError> {
        event_details_to_value(&self.0.content)
    }
}
