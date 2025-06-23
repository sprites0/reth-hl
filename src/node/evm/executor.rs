use super::config::HlBlockExecutionCtx;
use super::patch::patch_mainnet_after_tx;
use crate::{
    evm::transaction::HlTxEnv,
    hardforks::HlHardforks,
    node::types::{ReadPrecompileCalls, ReadPrecompileInput, ReadPrecompileResult},
};
use alloy_consensus::{Transaction, TxReceipt};
use alloy_eips::{eip7685::Requests, Encodable2718};
use alloy_evm::{block::ExecutableTx, eth::receipt_builder::ReceiptBuilderCtx};
use alloy_primitives::Bytes;
use reth_chainspec::{EthChainSpec, EthereumHardforks, Hardforks};
use reth_evm::{
    block::{BlockValidationError, CommitChanges},
    eth::receipt_builder::ReceiptBuilder,
    execute::{BlockExecutionError, BlockExecutor},
    precompiles::{DynPrecompile, PrecompilesMap},
    Database, Evm, FromRecoveredTx, FromTxWithEncoded, IntoTxEnv, OnStateHook, RecoveredTx,
};
use reth_primitives::TransactionSigned;
use reth_provider::BlockExecutionResult;
use reth_revm::State;
use revm::{
    context::{
        result::{ExecutionResult, ResultAndState},
        TxEnv,
    }, precompile::{PrecompileError, PrecompileOutput, PrecompileResult}, primitives::HashMap, DatabaseCommit
};

pub fn is_system_transaction(tx: &TransactionSigned) -> bool {
    let Some(gas_price) = tx.gas_price() else {
        return false;
    };
    gas_price == 0
}

pub struct HlBlockExecutor<'a, EVM, Spec, R: ReceiptBuilder>
where
    Spec: EthChainSpec,
{
    /// Reference to the specification object.
    spec: Spec,
    /// Inner EVM.
    evm: EVM,
    /// Gas used in the block.
    gas_used: u64,
    /// Receipts of executed transactions.
    receipts: Vec<R::Receipt>,
    /// System txs
    system_txs: Vec<R::Transaction>,
    /// Read precompile calls
    read_precompile_calls: ReadPrecompileCalls,
    /// Receipt builder.
    receipt_builder: R,
    /// Context for block execution.
    ctx: HlBlockExecutionCtx<'a>,
}

fn run_precompile(
    precompile_calls: &HashMap<ReadPrecompileInput, ReadPrecompileResult>,
    data: &[u8],
    gas_limit: u64,
) -> PrecompileResult {
    let input = ReadPrecompileInput {
        input: Bytes::copy_from_slice(data),
        gas_limit,
    };
    let Some(get) = precompile_calls.get(&input) else {
        return Err(PrecompileError::OutOfGas);
    };

    return match *get {
        ReadPrecompileResult::Ok {
            gas_used,
            ref bytes,
        } => {
            Ok(PrecompileOutput {
                gas_used,
                bytes: bytes.clone(),
            })
        }
        ReadPrecompileResult::OutOfGas => {
            // Use all the gas passed to this precompile
            Err(PrecompileError::OutOfGas)
        }
        ReadPrecompileResult::Error => {
            Err(PrecompileError::OutOfGas)
        }
        ReadPrecompileResult::UnexpectedError => panic!("unexpected precompile error"),
    };
}

impl<'a, DB, EVM, Spec, R: ReceiptBuilder> HlBlockExecutor<'a, EVM, Spec, R>
where
    DB: Database + 'a,
    EVM: Evm<
        DB = &'a mut State<DB>,
        Precompiles = PrecompilesMap,
        Tx: FromRecoveredTx<R::Transaction>
                + FromRecoveredTx<TransactionSigned>
                + FromTxWithEncoded<TransactionSigned>,
    >,
    Spec: EthereumHardforks + HlHardforks + EthChainSpec + Hardforks + Clone,
    R: ReceiptBuilder<Transaction = TransactionSigned, Receipt: TxReceipt>,
    <R as ReceiptBuilder>::Transaction: Unpin + From<TransactionSigned>,
    <EVM as alloy_evm::Evm>::Tx: FromTxWithEncoded<<R as ReceiptBuilder>::Transaction>,
    HlTxEnv<TxEnv>: IntoTxEnv<<EVM as alloy_evm::Evm>::Tx>,
    R::Transaction: Into<TransactionSigned>,
{
    /// Creates a new HlBlockExecutor.
    pub fn new(mut evm: EVM, ctx: HlBlockExecutionCtx<'a>, spec: Spec, receipt_builder: R) -> Self {
        let precompiles_mut = evm.precompiles_mut();
        // For all precompile addresses just in case it's populated and not cleared
        // Clear 0x00...08xx addresses
        let addresses = precompiles_mut.addresses().cloned().collect::<Vec<_>>();
        for address in addresses {
            if address.starts_with(&[0u8; 18]) && address[19] == 8 {
                precompiles_mut.apply_precompile(&address, |_| None);
            }
        }
        for (address, precompile) in ctx.read_precompile_calls.iter() {
            let precompile = precompile.clone();
            precompiles_mut.apply_precompile(address, |_| {
                Some(DynPrecompile::from(move |data: &[u8], gas: u64| {
                    run_precompile(&precompile, data, gas)
                }))
            });
        }
        Self {
            spec,
            evm,
            gas_used: 0,
            receipts: vec![],
            system_txs: vec![],
            read_precompile_calls: ctx.read_precompile_calls.clone().into(),
            receipt_builder,
            // system_contracts,
            ctx,
        }
    }
}

impl<'a, DB, E, Spec, R> BlockExecutor for HlBlockExecutor<'a, E, Spec, R>
where
    DB: Database + 'a,
    E: Evm<
        DB = &'a mut State<DB>,
        Tx: FromRecoveredTx<R::Transaction>
                + FromRecoveredTx<TransactionSigned>
                + FromTxWithEncoded<TransactionSigned>,
    >,
    Spec: EthereumHardforks + HlHardforks + EthChainSpec + Hardforks,
    R: ReceiptBuilder<Transaction = TransactionSigned, Receipt: TxReceipt>,
    <R as ReceiptBuilder>::Transaction: Unpin + From<TransactionSigned>,
    <E as alloy_evm::Evm>::Tx: FromTxWithEncoded<<R as ReceiptBuilder>::Transaction>,
    HlTxEnv<TxEnv>: IntoTxEnv<<E as alloy_evm::Evm>::Tx>,
    R::Transaction: Into<TransactionSigned>,
{
    type Transaction = TransactionSigned;
    type Receipt = R::Receipt;
    type Evm = E;

    fn apply_pre_execution_changes(&mut self) -> Result<(), BlockExecutionError> {
        Ok(())
    }

    fn execute_transaction_with_commit_condition(
        &mut self,
        _tx: impl ExecutableTx<Self>,
        _f: impl FnOnce(&ExecutionResult<<Self::Evm as Evm>::HaltReason>) -> CommitChanges,
    ) -> Result<Option<u64>, BlockExecutionError> {
        Ok(Some(0))
    }

    fn execute_transaction_with_result_closure(
        &mut self,
        tx: impl ExecutableTx<Self>
            + IntoTxEnv<<E as alloy_evm::Evm>::Tx>
            + RecoveredTx<TransactionSigned>,
        f: impl for<'b> FnOnce(&'b ExecutionResult<<E as alloy_evm::Evm>::HaltReason>),
    ) -> Result<u64, BlockExecutionError> {
        // Check if it's a system transaction
        // let signer = tx.signer();
        // let is_system_transaction = is_system_transaction(tx.tx());

        let block_available_gas = self.evm.block().gas_limit - self.gas_used;
        if tx.tx().gas_limit() > block_available_gas {
            return Err(
                BlockValidationError::TransactionGasLimitMoreThanAvailableBlockGas {
                    transaction_gas_limit: tx.tx().gas_limit(),
                    block_available_gas,
                }
                .into(),
            );
        }
        let result_and_state = self
            .evm
            .transact(tx)
            .map_err(|err| BlockExecutionError::evm(err, tx.tx().trie_hash()))?;
        let ResultAndState { result, mut state } = result_and_state;
        f(&result);
        let gas_used = result.gas_used();
        if !is_system_transaction(tx.tx()) {
            self.gas_used += gas_used;
        }

        // apply patches after
        patch_mainnet_after_tx(
            self.evm.block().number,
            self.receipts.len() as u64,
            is_system_transaction(tx.tx()),
            &mut state,
        )?;

        self.receipts
            .push(self.receipt_builder.build_receipt(ReceiptBuilderCtx {
                tx: tx.tx(),
                evm: &self.evm,
                result,
                state: &state,
                cumulative_gas_used: self.gas_used,
            }));

        self.evm.db_mut().commit(state);

        Ok(gas_used)
    }

    fn finish(self) -> Result<(Self::Evm, BlockExecutionResult<R::Receipt>), BlockExecutionError> {
        Ok((
            self.evm,
            BlockExecutionResult {
                receipts: self.receipts,
                requests: Requests::default(),
                gas_used: self.gas_used,
            },
        ))
    }

    fn set_state_hook(&mut self, _hook: Option<Box<dyn OnStateHook>>) {}

    fn evm_mut(&mut self) -> &mut Self::Evm {
        &mut self.evm
    }

    fn evm(&self) -> &Self::Evm {
        &self.evm
    }
}
