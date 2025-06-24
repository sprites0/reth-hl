//! EVM Handler related to Hl chain

use super::{spec::HlSpecId, transaction::HlTxTr};
use revm::{
    context::{
        result::{ExecutionResult, HaltReason},
        Cfg, ContextTr, JournalOutput, LocalContextTr,
    },
    context_interface::{result::ResultAndState, JournalTr},
    handler::{handler::EvmTrError, EvmTr, Frame, FrameResult, Handler, MainnetHandler},
    inspector::{Inspector, InspectorEvmTr, InspectorFrame, InspectorHandler},
    interpreter::{interpreter::EthInterpreter, FrameInput, SuccessOrHalt},
};

pub struct HlHandler<EVM, ERROR, FRAME> {
    pub mainnet: MainnetHandler<EVM, ERROR, FRAME>,
}

impl<EVM, ERROR, FRAME> HlHandler<EVM, ERROR, FRAME> {
    pub fn new() -> Self {
        Self {
            mainnet: MainnetHandler::default(),
        }
    }
}

impl<EVM, ERROR, FRAME> Default for HlHandler<EVM, ERROR, FRAME> {
    fn default() -> Self {
        Self::new()
    }
}

pub trait HlContextTr:
    ContextTr<Journal: JournalTr<FinalOutput = JournalOutput>, Tx: HlTxTr, Cfg: Cfg<Spec = HlSpecId>>
{
}

impl<T> HlContextTr for T where
    T: ContextTr<
        Journal: JournalTr<FinalOutput = JournalOutput>,
        Tx: HlTxTr,
        Cfg: Cfg<Spec = HlSpecId>,
    >
{
}

impl<EVM, ERROR, FRAME> Handler for HlHandler<EVM, ERROR, FRAME>
where
    EVM: EvmTr<Context: HlContextTr>,
    ERROR: EvmTrError<EVM>,
    FRAME: Frame<Evm = EVM, Error = ERROR, FrameResult = FrameResult, FrameInit = FrameInput>,
{
    type Evm = EVM;
    type Error = ERROR;
    type Frame = FRAME;
    type HaltReason = HaltReason;

    fn validate_initial_tx_gas(
        &self,
        evm: &Self::Evm,
    ) -> Result<revm::interpreter::InitialAndFloorGas, Self::Error> {
        self.mainnet.validate_initial_tx_gas(evm)
    }

    fn output(
        &self,
        evm: &mut Self::Evm,
        result: <Self::Frame as Frame>::FrameResult,
    ) -> Result<ResultAndState<Self::HaltReason>, Self::Error> {
        let ctx = evm.ctx();
        ctx.error();

        // used gas with refund calculated.
        let gas_refunded = result.gas().refunded() as u64;
        let final_gas_used = result.gas().spent() - gas_refunded;
        let output = result.output();
        let instruction_result = result.into_interpreter_result();

        // Reset journal and return present state.
        let JournalOutput { state, logs } = evm.ctx().journal().finalize();

        let result = match SuccessOrHalt::from(instruction_result.result) {
            SuccessOrHalt::Success(reason) => ExecutionResult::Success {
                reason,
                gas_used: final_gas_used,
                gas_refunded,
                logs,
                output,
            },
            SuccessOrHalt::Revert => ExecutionResult::Revert {
                gas_used: final_gas_used,
                output: output.into_data(),
            },
            SuccessOrHalt::Halt(reason) => ExecutionResult::Halt {
                reason,
                gas_used: final_gas_used,
            },
            // Only two internal return flags.
            flag @ (SuccessOrHalt::FatalExternalError | SuccessOrHalt::Internal(_)) => {
                panic!(
                "Encountered unexpected internal return flag: {flag:?} with instruction result: {instruction_result:?}"
            )
            }
        };

        // Clear local context
        evm.ctx().local().clear();
        // Clear journal
        evm.ctx().journal().clear();

        Ok(ResultAndState { result, state })
    }
}

impl<EVM, ERROR, FRAME> InspectorHandler for HlHandler<EVM, ERROR, FRAME>
where
    EVM: InspectorEvmTr<
        Context: HlContextTr,
        Inspector: Inspector<<<Self as Handler>::Evm as EvmTr>::Context, EthInterpreter>,
    >,
    ERROR: EvmTrError<EVM>,
    FRAME: Frame<Evm = EVM, Error = ERROR, FrameResult = FrameResult, FrameInit = FrameInput>
        + InspectorFrame<IT = EthInterpreter>,
{
    type IT = EthInterpreter;
}
