#![allow(unused)]

use crate::node::types::ReadPrecompileResult;
use crate::node::types::{ReadPrecompileInput, ReadPrecompileMap};
use revm::interpreter::{Gas, InstructionResult};
use revm::precompile::PrecompileError;
use revm::precompile::PrecompileOutput;
use revm::primitives::Bytes;

use super::spec::HlSpecId;
use cfg_if::cfg_if;
use once_cell::{race::OnceBox, sync::Lazy};
use revm::{
    context::Cfg,
    context_interface::ContextTr,
    handler::{EthPrecompiles, PrecompileProvider},
    interpreter::{InputsImpl, InterpreterResult},
    precompile::{bls12_381, kzg_point_evaluation, modexp, secp256r1, Precompiles},
    primitives::{hardfork::SpecId, Address},
};
use std::boxed::Box;

// HL precompile provider
#[derive(Debug, Clone)]
pub struct HlPrecompiles {
    /// Inner precompile provider is same as Ethereums.
    inner: EthPrecompiles,
    /// Precompile calls
    precompile_calls: ReadPrecompileMap,
}

impl HlPrecompiles {
    /// Create a new precompile provider with the given hl spec.
    #[inline]
    pub fn new(spec: HlSpecId, precompile_calls: ReadPrecompileMap) -> Self {
        let precompiles = cancun();

        Self {
            inner: EthPrecompiles {
                precompiles,
                spec: spec.into_eth_spec(),
            },
            precompile_calls,
        }
    }

    #[inline]
    pub fn precompiles(&self) -> &'static Precompiles {
        self.inner.precompiles
    }
}

/// Returns precompiles for Istanbul spec.
pub fn cancun() -> &'static Precompiles {
    static INSTANCE: OnceBox<Precompiles> = OnceBox::new();
    INSTANCE.get_or_init(|| {
        let mut precompiles = Precompiles::cancun().clone();
        // precompiles.extend([tendermint::TENDERMINT_HEADER_VALIDATION, iavl::IAVL_PROOF_VALIDATION]);
        Box::new(precompiles)
    })
}

impl<CTX> PrecompileProvider<CTX> for HlPrecompiles
where
    CTX: ContextTr<Cfg: Cfg<Spec = HlSpecId>>,
{
    type Output = InterpreterResult;

    #[inline]
    fn set_spec(&mut self, spec: <CTX::Cfg as Cfg>::Spec) -> bool {
        *self = Self::new(spec, self.precompile_calls.clone());
        true
    }

    #[inline]
    fn run(
        &mut self,
        context: &mut CTX,
        address: &Address,
        inputs: &InputsImpl,
        is_static: bool,
        gas_limit: u64,
    ) -> Result<Option<Self::Output>, String> {
        let Some(precompile_calls) = self.precompile_calls.get(address) else {
            return self
                .inner
                .run(context, address, inputs, is_static, gas_limit);
        };

        let input = ReadPrecompileInput {
            input: inputs.input.bytes(context),
            gas_limit,
        };
        let mut result = InterpreterResult {
            result: InstructionResult::Return,
            gas: Gas::new(gas_limit),
            output: Bytes::new(),
        };

        let Some(get) = precompile_calls.get(&input) else {
            result.gas.spend_all();
            result.result = InstructionResult::PrecompileError;
            return Ok(Some(result));
        };

        return match *get {
            ReadPrecompileResult::Ok {
                gas_used,
                ref bytes,
            } => {
                let underflow = result.gas.record_cost(gas_used);
                assert!(underflow, "Gas underflow is not possible");
                result.output = bytes.clone();
                Ok(Some(result))
            }
            ReadPrecompileResult::OutOfGas => {
                // Use all the gas passed to this precompile
                result.gas.spend_all();
                result.result = InstructionResult::OutOfGas;
                Ok(Some(result))
            }
            ReadPrecompileResult::Error => {
                result.gas.spend_all();
                result.result = InstructionResult::PrecompileError;
                Ok(Some(result))
            }
            ReadPrecompileResult::UnexpectedError => panic!("unexpected precompile error"),
        };
    }

    #[inline]
    fn warm_addresses(&self) -> Box<impl Iterator<Item = Address>> {
        self.inner.warm_addresses()
    }

    #[inline]
    fn contains(&self, address: &Address) -> bool {
        self.inner.contains(address)
    }
}

impl Default for HlPrecompiles {
    fn default() -> Self {
        Self::new(HlSpecId::default(), ReadPrecompileMap::default())
    }
}
