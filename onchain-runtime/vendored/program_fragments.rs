#[macro_export]
macro_rules! kernel_claim_zswap_nullifier {
  ($f:expr_2021, $fcached:expr_2021, $nul:expr_2021) => {
    [
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Idx { cached: true.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($nul.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Null.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: 2.try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
    ]
  };
}
pub use kernel_claim_zswap_nullifier;
#[macro_export]
macro_rules! kernel_claim_zswap_coin_spend {
  ($f:expr_2021, $fcached:expr_2021, $note:expr_2021) => {
    [
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Idx { cached: true.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(2 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($note.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Null.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: 2.try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
    ]
  };
}
pub use kernel_claim_zswap_coin_spend;
#[macro_export]
macro_rules! kernel_claim_zswap_coin_receive {
  ($f:expr_2021, $fcached:expr_2021, $note:expr_2021) => {
    [
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Idx { cached: true.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($note.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Null.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: 2.try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
    ]
  };
}
pub use kernel_claim_zswap_coin_receive;
#[macro_export]
macro_rules! kernel_claim_contract_call {
  ($f:expr_2021, $fcached:expr_2021, $addr:expr_2021, $entry_point:expr_2021, $comm:expr_2021) => {
    [
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Idx { cached: true.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(3 as u8).into())].try_into().unwrap() },
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Size,
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::concat([AlignedValue::from($addr.clone()), AlignedValue::from($entry_point.clone()), AlignedValue::from($comm.clone())].iter()).try_into().unwrap())).try_into().unwrap() },
      Op::Concat { cached: true.try_into().unwrap(), n: 160.try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Null.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: 2.try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
    ]
  };
}
pub use kernel_claim_contract_call;
#[macro_export]
macro_rules! kernel_checkpoint {
  ($f:expr_2021, $fcached:expr_2021) => {
    [
      Op::Ckpt,
    ]
  };
}
pub use kernel_checkpoint;
#[macro_export]
macro_rules! kernel_mint {
  ($f:expr_2021, $fcached:expr_2021, $domain_sep:expr_2021, $amount:expr_2021) => {
    [
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Idx { cached: true.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(4 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($domain_sep.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Dup { n: 1.try_into().unwrap() },
      Op::Dup { n: 1.try_into().unwrap() },
      Op::Member,
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($amount.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Neg,
      Op::Branch { skip: 4.try_into().unwrap() },
      Op::Dup { n: 2.try_into().unwrap() },
      Op::Dup { n: 2.try_into().unwrap() },
      Op::Idx { cached: true.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Stack].try_into().unwrap() },
      Op::Add,
      Op::Ins { cached: true.try_into().unwrap(), n: 2.try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
    ]
  };
}
pub use kernel_mint;
#[macro_export]
macro_rules! kernel_self {
  ($f:expr_2021, $fcached:expr_2021) => {
    [
      Op::Dup { n: 2.try_into().unwrap() },
      Op::Idx { cached: true.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use kernel_self;
#[macro_export]
macro_rules! Cell_read {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Popeq { cached: $fcached.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use Cell_read;
#[macro_export]
macro_rules! Cell_write {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty, $value:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().iter().cloned().rev().collect::<Vec<_>>().iter().cloned().skip(1).collect::<Vec<_>>().iter().cloned().rev().collect::<Vec<_>>().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($f.clone().iter().cloned().rev().collect::<Vec<_>>()[0].clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Cell(Sp::new($value.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) - 1).try_into().unwrap() },
    ]
  };
}
pub use Cell_write;
#[macro_export]
macro_rules! Cell_reset_to_default {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().iter().cloned().rev().collect::<Vec<_>>().iter().cloned().skip(1).collect::<Vec<_>>().iter().cloned().rev().collect::<Vec<_>>().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($f.clone().iter().cloned().rev().collect::<Vec<_>>()[0].clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from(<$value_type>::default()).try_into().unwrap())).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) - 1).try_into().unwrap() },
    ]
  };
}
pub use Cell_reset_to_default;
#[macro_export]
macro_rules! Cell_write_coin {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty, $coin:expr_2021, $recipient:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().iter().cloned().rev().collect::<Vec<_>>().iter().cloned().skip(1).collect::<Vec<_>>().iter().cloned().rev().collect::<Vec<_>>().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($f.clone().iter().cloned().rev().collect::<Vec<_>>()[0].clone().try_into().unwrap())).try_into().unwrap() },
      Op::Dup { n: (3 + ((($f.clone().len() as u8) - 1) * 2)).try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($coin.clone().commitment(&$recipient.clone()).try_into().unwrap())).try_into().unwrap() },
      Op::Idx { cached: true.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into()), Key::Stack].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($coin.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Concat { cached: true.try_into().unwrap(), n: 91.try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) - 1).try_into().unwrap() },
    ]
  };
}
pub use Cell_write_coin;
#[macro_export]
macro_rules! Counter_read {
  ($f:expr_2021, $fcached:expr_2021) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use Counter_read;
#[macro_export]
macro_rules! Counter_less_than {
  ($f:expr_2021, $fcached:expr_2021, $threshold:expr_2021) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($threshold.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Lt,
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use Counter_less_than;
#[macro_export]
macro_rules! Counter_increment {
  ($f:expr_2021, $fcached:expr_2021, $amount:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Addi { immediate: u32::try_from($amount.clone()).unwrap().try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: ($f.clone().len() as u8).try_into().unwrap() },
    ]
  };
}
pub use Counter_increment;
#[macro_export]
macro_rules! Counter_decrement {
  ($f:expr_2021, $fcached:expr_2021, $amount:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Subi { immediate: u32::try_from($amount.clone()).unwrap().try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: ($f.clone().len() as u8).try_into().unwrap() },
    ]
  };
}
pub use Counter_decrement;
#[macro_export]
macro_rules! Counter_reset_to_default {
  ($f:expr_2021, $fcached:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().iter().cloned().rev().collect::<Vec<_>>().iter().cloned().skip(1).collect::<Vec<_>>().iter().cloned().rev().collect::<Vec<_>>().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($f.clone().iter().cloned().rev().collect::<Vec<_>>()[0].clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from(0 as u64).try_into().unwrap())).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) - 1).try_into().unwrap() },
    ]
  };
}
pub use Counter_reset_to_default;
#[macro_export]
macro_rules! Set_reset_to_default {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().iter().cloned().rev().collect::<Vec<_>>().iter().cloned().skip(1).collect::<Vec<_>>().iter().cloned().rev().collect::<Vec<_>>().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($f.clone().iter().cloned().rev().collect::<Vec<_>>()[0].clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Map([].iter().cloned().collect()).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) - 1).try_into().unwrap() },
    ]
  };
}
pub use Set_reset_to_default;
#[macro_export]
macro_rules! Set_is_empty {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Size,
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from(0 as u64).try_into().unwrap())).try_into().unwrap() },
      Op::Eq,
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use Set_is_empty;
#[macro_export]
macro_rules! Set_size {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Size,
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use Set_size;
#[macro_export]
macro_rules! Set_member {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty, $elem:expr_2021) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($elem.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Member,
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use Set_member;
#[macro_export]
macro_rules! Set_insert {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty, $elem:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($elem.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Null.try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: ($f.clone().len() as u8).try_into().unwrap() },
    ]
  };
}
pub use Set_insert;
#[macro_export]
macro_rules! Set_remove {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty, $elem:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($elem.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Rem { cached: false.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: ($f.clone().len() as u8).try_into().unwrap() },
    ]
  };
}
pub use Set_remove;
#[macro_export]
macro_rules! Set_insert_coin {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty, $coin:expr_2021, $recipient:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Dup { n: (2 + (($f.clone().len() as u8) * 2)).try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($coin.clone().commitment(&$recipient.clone()).try_into().unwrap())).try_into().unwrap() },
      Op::Idx { cached: true.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into()), Key::Stack].try_into().unwrap() },
      Op::Concat { cached: true.try_into().unwrap(), n: 91.try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($coin.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Null.try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: ($f.clone().len() as u8).try_into().unwrap() },
    ]
  };
}
pub use Set_insert_coin;
#[macro_export]
macro_rules! Map_reset_to_default {
  ($f:expr_2021, $fcached:expr_2021, $key_type:ty, $value_type:ty) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().iter().cloned().rev().collect::<Vec<_>>().iter().cloned().skip(1).collect::<Vec<_>>().iter().cloned().rev().collect::<Vec<_>>().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($f.clone().iter().cloned().rev().collect::<Vec<_>>()[0].clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Map([].iter().cloned().collect()).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) - 1).try_into().unwrap() },
    ]
  };
}
pub use Map_reset_to_default;
#[macro_export]
macro_rules! Map_is_empty {
  ($f:expr_2021, $fcached:expr_2021, $key_type:ty, $value_type:ty) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Size,
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from(0 as u64).try_into().unwrap())).try_into().unwrap() },
      Op::Eq,
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use Map_is_empty;
#[macro_export]
macro_rules! Map_size {
  ($f:expr_2021, $fcached:expr_2021, $key_type:ty, $value_type:ty) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Size,
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use Map_size;
#[macro_export]
macro_rules! Map_member {
  ($f:expr_2021, $fcached:expr_2021, $key_type:ty, $value_type:ty, $key:expr_2021) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($key.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Member,
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use Map_member;
#[macro_export]
macro_rules! Map_lookup {
  ($f:expr_2021, $fcached:expr_2021, $key_type:ty, $value_type:ty, $key:expr_2021) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value($key.clone().into())].try_into().unwrap() },
      Op::Popeq { cached: false.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use Map_lookup;
#[macro_export]
macro_rules! Map_insert {
  ($f:expr_2021, $fcached:expr_2021, $key_type:ty, $value_type:ty, $key:expr_2021, $value:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($key.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::from($value.clone()).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: ($f.clone().len() as u8).try_into().unwrap() },
    ]
  };
}
pub use Map_insert;
#[macro_export]
macro_rules! Map_insert_default {
  ($f:expr_2021, $fcached:expr_2021, $key_type:ty, $value_type:ty, $key:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($key.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::from(AlignedValue::from(<$value_type>::default())).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: ($f.clone().len() as u8).try_into().unwrap() },
    ]
  };
}
pub use Map_insert_default;
#[macro_export]
macro_rules! Map_remove {
  ($f:expr_2021, $fcached:expr_2021, $key_type:ty, $value_type:ty, $key:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($key.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Rem { cached: false.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: ($f.clone().len() as u8).try_into().unwrap() },
    ]
  };
}
pub use Map_remove;
#[macro_export]
macro_rules! Map_insert_coin {
  ($f:expr_2021, $fcached:expr_2021, $key_type:ty, $value_type:ty, $key:expr_2021, $coin:expr_2021, $recipient:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($key.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Dup { n: (2 + (($f.clone().len() as u8) * 2)).try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($coin.clone().commitment(&$recipient.clone()).try_into().unwrap())).try_into().unwrap() },
      Op::Idx { cached: true.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into()), Key::Stack].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($coin.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Concat { cached: true.try_into().unwrap(), n: 91.try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: ($f.clone().len() as u8).try_into().unwrap() },
    ]
  };
}
pub use Map_insert_coin;
#[macro_export]
macro_rules! List_reset_to_default {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().iter().cloned().rev().collect::<Vec<_>>().iter().cloned().skip(1).collect::<Vec<_>>().iter().cloned().rev().collect::<Vec<_>>().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($f.clone().iter().cloned().rev().collect::<Vec<_>>()[0].clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Array(vec![StateValue::Null, StateValue::Null, StateValue::Cell(Sp::new(AlignedValue::from(0 as u64).try_into().unwrap()))].into()).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) - 1).try_into().unwrap() },
    ]
  };
}
pub use List_reset_to_default;
#[macro_export]
macro_rules! List_is_empty {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Type,
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from(1 as u8).try_into().unwrap())).try_into().unwrap() },
      Op::Eq,
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use List_is_empty;
#[macro_export]
macro_rules! List_length {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(2 as u8).into())].try_into().unwrap() },
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use List_length;
#[macro_export]
macro_rules! List_head {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Type,
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from(1 as u8).try_into().unwrap())).try_into().unwrap() },
      Op::Eq,
      Op::Branch { skip: 4.try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from(1 as u8).try_into().unwrap())).try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Concat { cached: false.try_into().unwrap(), n: (2 + (<$value_type>::alignment().max_aligned_size() as u32)).try_into().unwrap() },
      Op::Jmp { skip: 2.try_into().unwrap() },
      Op::Pop,
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::concat([AlignedValue::from(AlignedValue::from(0 as u8)), AlignedValue::from(AlignedValue::from(<$value_type>::default()))].iter()).try_into().unwrap())).try_into().unwrap() },
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use List_head;
#[macro_export]
macro_rules! List_pop_front {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: ($f.clone().len() as u8).try_into().unwrap() },
    ]
  };
}
pub use List_pop_front;
#[macro_export]
macro_rules! List_push_front {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty, $value:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(2 as u8).into())].try_into().unwrap() },
      Op::Addi { immediate: 1.try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Array(vec![StateValue::Cell(Sp::new($value.clone().try_into().unwrap())), StateValue::Null, StateValue::Null].into()).try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from(2 as u8).try_into().unwrap())).try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from(1 as u8).try_into().unwrap())).try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) + 1).try_into().unwrap() },
    ]
  };
}
pub use List_push_front;
#[macro_export]
macro_rules! List_push_front_coin {
  ($f:expr_2021, $fcached:expr_2021, $value_type:ty, $coin:expr_2021, $recipient:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(2 as u8).into())].try_into().unwrap() },
      Op::Addi { immediate: 1.try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from(0 as u8).try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Array(vec![StateValue::Null, StateValue::Null, StateValue::Null].into()).try_into().unwrap() },
      Op::Dup { n: (4 + (($f.clone().len() as u8) * 2)).try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($coin.clone().commitment(&$recipient.clone()).try_into().unwrap())).try_into().unwrap() },
      Op::Idx { cached: true.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into()), Key::Stack].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($coin.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Concat { cached: true.try_into().unwrap(), n: 91.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from(2 as u8).try_into().unwrap())).try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from(1 as u8).try_into().unwrap())).try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) + 1).try_into().unwrap() },
    ]
  };
}
pub use List_push_front_coin;
#[macro_export]
macro_rules! MerkleTree_reset_to_default {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().iter().cloned().rev().collect::<Vec<_>>().iter().cloned().skip(1).collect::<Vec<_>>().iter().cloned().rev().collect::<Vec<_>>().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($f.clone().iter().cloned().rev().collect::<Vec<_>>()[0].clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Array(vec![StateValue::BoundedMerkleTree(MerkleTree::blank($nat)), StateValue::Cell(Sp::new(AlignedValue::from(0 as u64).try_into().unwrap()))].into()).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) - 1).try_into().unwrap() },
    ]
  };
}
pub use MerkleTree_reset_to_default;
#[macro_export]
macro_rules! MerkleTree_is_full {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from((2 as u64).pow($nat) as u64).try_into().unwrap())).try_into().unwrap() },
      Op::Lt,
      Op::Neg,
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use MerkleTree_is_full;
#[macro_export]
macro_rules! MerkleTree_check_root {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty, $rt:expr_2021) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Root,
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($rt.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Eq,
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use MerkleTree_check_root;
#[macro_export]
macro_rules! MerkleTree_insert {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty, $item:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Dup { n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Cell(Sp::new(leaf_hash(&ValueReprAlignedValue(AlignedValue::from($item.clone()))).try_into().unwrap())).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Addi { immediate: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) + 1).try_into().unwrap() },
    ]
  };
}
pub use MerkleTree_insert;
#[macro_export]
macro_rules! MerkleTree_insert_index {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty, $item:expr_2021, $index:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($index.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Cell(Sp::new(leaf_hash(&ValueReprAlignedValue(AlignedValue::from($item.clone()))).try_into().unwrap())).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($index.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Addi { immediate: 1.try_into().unwrap() },
      Op::Dup { n: 1.try_into().unwrap() },
      Op::Dup { n: 1.try_into().unwrap() },
      Op::Lt,
      Op::Branch { skip: 2.try_into().unwrap() },
      Op::Pop,
      Op::Jmp { skip: 2.try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Pop,
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: ($f.clone().len() as u8).try_into().unwrap() },
    ]
  };
}
pub use MerkleTree_insert_index;
#[macro_export]
macro_rules! MerkleTree_insert_hash {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty, $hash:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Dup { n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Cell(Sp::new($hash.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Addi { immediate: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) + 1).try_into().unwrap() },
    ]
  };
}
pub use MerkleTree_insert_hash;
#[macro_export]
macro_rules! MerkleTree_insert_hash_index {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty, $hash:expr_2021, $index:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($index.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Cell(Sp::new($hash.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($index.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Addi { immediate: 1.try_into().unwrap() },
      Op::Dup { n: 1.try_into().unwrap() },
      Op::Dup { n: 1.try_into().unwrap() },
      Op::Lt,
      Op::Branch { skip: 2.try_into().unwrap() },
      Op::Pop,
      Op::Jmp { skip: 2.try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Pop,
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: ($f.clone().len() as u8).try_into().unwrap() },
    ]
  };
}
pub use MerkleTree_insert_hash_index;
#[macro_export]
macro_rules! MerkleTree_insert_index_default {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty, $index:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($index.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Cell(Sp::new(leaf_hash(&ValueReprAlignedValue(AlignedValue::from(AlignedValue::from(<$value_type>::default())))).try_into().unwrap())).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($index.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Addi { immediate: 1.try_into().unwrap() },
      Op::Dup { n: 1.try_into().unwrap() },
      Op::Dup { n: 1.try_into().unwrap() },
      Op::Lt,
      Op::Branch { skip: 2.try_into().unwrap() },
      Op::Pop,
      Op::Jmp { skip: 2.try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Pop,
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: ($f.clone().len() as u8).try_into().unwrap() },
    ]
  };
}
pub use MerkleTree_insert_index_default;
#[macro_export]
macro_rules! HistoricMerkleTree_reset_to_default {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().iter().cloned().rev().collect::<Vec<_>>().iter().cloned().skip(1).collect::<Vec<_>>().iter().cloned().rev().collect::<Vec<_>>().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($f.clone().iter().cloned().rev().collect::<Vec<_>>()[0].clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Array(vec![StateValue::BoundedMerkleTree(MerkleTree::blank($nat)), StateValue::Cell(Sp::new(AlignedValue::from(0 as u64).try_into().unwrap())), StateValue::Map([].iter().cloned().collect())].into()).try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(2 as u8).into())].try_into().unwrap() },
      Op::Dup { n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Root,
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Null.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: 2.try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) - 1).try_into().unwrap() },
    ]
  };
}
pub use HistoricMerkleTree_reset_to_default;
#[macro_export]
macro_rules! HistoricMerkleTree_is_full {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from((2 as u64).pow($nat) as u64).try_into().unwrap())).try_into().unwrap() },
      Op::Lt,
      Op::Neg,
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use HistoricMerkleTree_is_full;
#[macro_export]
macro_rules! HistoricMerkleTree_check_root {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty, $rt:expr_2021) => {
    [
      Op::Dup { n: 0.try_into().unwrap() },
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: false.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(2 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($rt.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Member,
      Op::Popeq { cached: true.try_into().unwrap(), result: ().try_into().unwrap() },
    ]
  };
}
pub use HistoricMerkleTree_check_root;
#[macro_export]
macro_rules! HistoricMerkleTree_insert {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty, $item:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Dup { n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Cell(Sp::new(leaf_hash(&ValueReprAlignedValue(AlignedValue::from($item.clone()))).try_into().unwrap())).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Addi { immediate: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(2 as u8).into())].try_into().unwrap() },
      Op::Dup { n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Root,
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Null.try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) + 1).try_into().unwrap() },
    ]
  };
}
pub use HistoricMerkleTree_insert;
#[macro_export]
macro_rules! HistoricMerkleTree_insert_index {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty, $item:expr_2021, $index:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($index.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Cell(Sp::new(leaf_hash(&ValueReprAlignedValue(AlignedValue::from($item.clone()))).try_into().unwrap())).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($index.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Addi { immediate: 1.try_into().unwrap() },
      Op::Dup { n: 1.try_into().unwrap() },
      Op::Dup { n: 1.try_into().unwrap() },
      Op::Lt,
      Op::Branch { skip: 2.try_into().unwrap() },
      Op::Pop,
      Op::Jmp { skip: 2.try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Pop,
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(2 as u8).into())].try_into().unwrap() },
      Op::Dup { n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Root,
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Null.try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) + 1).try_into().unwrap() },
    ]
  };
}
pub use HistoricMerkleTree_insert_index;
#[macro_export]
macro_rules! HistoricMerkleTree_insert_hash {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty, $hash:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Dup { n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Cell(Sp::new($hash.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Addi { immediate: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(2 as u8).into())].try_into().unwrap() },
      Op::Dup { n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Root,
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Null.try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) + 1).try_into().unwrap() },
    ]
  };
}
pub use HistoricMerkleTree_insert_hash;
#[macro_export]
macro_rules! HistoricMerkleTree_insert_hash_index {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty, $hash:expr_2021, $index:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($index.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Cell(Sp::new($hash.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($index.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Addi { immediate: 1.try_into().unwrap() },
      Op::Dup { n: 1.try_into().unwrap() },
      Op::Dup { n: 1.try_into().unwrap() },
      Op::Lt,
      Op::Branch { skip: 2.try_into().unwrap() },
      Op::Pop,
      Op::Jmp { skip: 2.try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Pop,
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(2 as u8).into())].try_into().unwrap() },
      Op::Dup { n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Root,
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Null.try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) + 1).try_into().unwrap() },
    ]
  };
}
pub use HistoricMerkleTree_insert_hash_index;
#[macro_export]
macro_rules! HistoricMerkleTree_insert_index_default {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty, $index:expr_2021) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($index.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Cell(Sp::new(leaf_hash(&ValueReprAlignedValue(AlignedValue::from(AlignedValue::from(<$value_type>::default())))).try_into().unwrap())).try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(1 as u8).into())].try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new($index.clone().try_into().unwrap())).try_into().unwrap() },
      Op::Addi { immediate: 1.try_into().unwrap() },
      Op::Dup { n: 1.try_into().unwrap() },
      Op::Dup { n: 1.try_into().unwrap() },
      Op::Lt,
      Op::Branch { skip: 2.try_into().unwrap() },
      Op::Pop,
      Op::Jmp { skip: 2.try_into().unwrap() },
      Op::Swap { n: 0.try_into().unwrap() },
      Op::Pop,
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: true.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(2 as u8).into())].try_into().unwrap() },
      Op::Dup { n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Root,
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Null.try_into().unwrap() },
      Op::Ins { cached: false.try_into().unwrap(), n: 1.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) + 1).try_into().unwrap() },
    ]
  };
}
pub use HistoricMerkleTree_insert_index_default;
#[macro_export]
macro_rules! HistoricMerkleTree_reset_history {
  ($f:expr_2021, $fcached:expr_2021, $nat:literal, $value_type:ty) => {
    [
      Op::Idx { cached: $fcached.try_into().unwrap(), push_path: true.try_into().unwrap(), path: $f.clone().try_into().unwrap() },
      Op::Push { storage: false.try_into().unwrap(), value: StateValue::Cell(Sp::new(AlignedValue::from(2 as u8).try_into().unwrap())).try_into().unwrap() },
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Map([].iter().cloned().collect()).try_into().unwrap() },
      Op::Dup { n: 2.try_into().unwrap() },
      Op::Idx { cached: false.try_into().unwrap(), push_path: false.try_into().unwrap(), path: vec![Key::Value(AlignedValue::from(0 as u8).into())].try_into().unwrap() },
      Op::Root,
      Op::Push { storage: true.try_into().unwrap(), value: StateValue::Null.try_into().unwrap() },
      Op::Ins { cached: true.try_into().unwrap(), n: (($f.clone().len() as u8) + 2).try_into().unwrap() },
    ]
  };
}
pub use HistoricMerkleTree_reset_history;
