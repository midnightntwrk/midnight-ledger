use std::ops::Add;

use midnight_circuits::instructions::{ArithInstructions, EccInstructions};
use midnight_proofs::{circuit::Layouter, plonk};
use midnight_zk_stdlib::ZkStdLib;

use crate::{
    ir_instructions::F,
    ir_types::{CircuitValue, IrValue},
};

/// Adds off-circuit the given inputs.
/// Addition is supported on:
///   - `Native`
///   - `JubjubPoint`
///
/// # Errors
///
/// This function results in an error if the input types are not supported.
pub fn add_offcircuit(x: &IrValue, y: &IrValue) -> Result<IrValue, anyhow::Error> {
    use IrValue::*;
    match (x, y) {
        (Native(a), Native(b)) => Ok(Native(*a + *b)),
        (JubjubPoint(p), JubjubPoint(q)) => Ok(JubjubPoint(p + q)),
        _ => Err(anyhow::anyhow!(
            "Unsupported addition: {:?} + {:?}",
            x.get_type(),
            y.get_type()
        )),
    }
}

/// Adds in-circuit the given inputs.
/// Addition is supported on:
///   - `Native`
///   - `JubjubPoint`
///
/// # Errors
///
/// This function results in an error if the input types are not supported.
pub fn add_incircuit(
    std_lib: &ZkStdLib,
    layouter: &mut impl Layouter<F>,
    x: &CircuitValue,
    y: &CircuitValue,
) -> Result<CircuitValue, plonk::Error> {
    use CircuitValue::*;
    match (x, y) {
        (Native(a), Native(b)) => {
            let r = std_lib.add(layouter, a, b)?;
            Ok(Native(r))
        }
        (JubjubPoint(p), JubjubPoint(q)) => {
            let r = std_lib.jubjub().add(layouter, p, q)?;
            Ok(JubjubPoint(r))
        }
        _ => Err(plonk::Error::Synthesis(format!(
            "Unsupported addition: {:?} + {:?}",
            x.get_type(),
            y.get_type()
        ))),
    }
}

impl Add for IrValue {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        add_offcircuit(&self, &rhs).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use group::Group;
    use group::ff::Field;
    use midnight_curves::JubjubSubgroup;
    use rand_chacha::rand_core::OsRng;
    use transient_crypto::curve::Fr;

    use super::*;

    #[test]
    fn test_add() {
        use IrValue::*;

        let [x, y] = core::array::from_fn(|_| Fr(F::random(OsRng)));
         let [p, q] = core::array::from_fn(|_| JubjubSubgroup::random(OsRng));

        assert_eq!(Native(x) + Native(y), Native(x + y));
        assert_eq!(JubjubPoint(p) + JubjubPoint(q), JubjubPoint(p + q));

        // Negative test: adding incompatible types should fail
        let result = add_offcircuit(&Native(x), &JubjubPoint(p));
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Unsupported addition: Native + JubjubPoint"
        );
    }
}
