#![allow(unused)]

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
}

impl HlPrecompiles {
    /// Create a new precompile provider with the given hl spec.
    #[inline]
    pub fn new(spec: HlSpecId) -> Self {
        let precompiles = cancun();

        Self { inner: EthPrecompiles { precompiles, spec: spec.into_eth_spec() } }
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
        *self = Self::new(spec);
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
        self.inner.run(context, address, inputs, is_static, gas_limit)
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
        Self::new(HlSpecId::default())
    }
}
