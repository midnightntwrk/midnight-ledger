use v8_to_v9_state_translation_replay::LedgerTest;

mod translation;

pub fn all() -> Vec<Box<dyn LedgerTest>> {
    vec![Box::new(translation::IncrementalTranslation)]
}
