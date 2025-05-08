#![warn(unused_crate_dependencies)]

use alloy_rlp::Encodable;
use alloy_rpc_types_debug::ExecutionWitness;
use reth_ethereum::{
    chainspec::ChainSpecBuilder,
    evm::{
        primitives::execute::Executor,
        revm::{State, database::StateProviderDatabase, witness::ExecutionWitnessRecord},
    },
    node::{EthExecutorProvider, EthereumNode, api::ConfigureEvm},
    provider::{BlockReader, ChainSpecProvider, HeaderProvider, providers::ReadOnlyConfig},
    storage::TransactionVariant,
};

fn main() -> eyre::Result<()> {
    // The path to data directory, e.g. "~/.local/reth/share/mainnet"
    let datadir = std::env::var("RETH_DATADIR")?;

    // Instantiate a provider factory for Ethereum mainnet using the provided datadir path.
    let spec = ChainSpecBuilder::mainnet().build();
    let factory = EthereumNode::provider_factory_builder()
        .open_read_only(spec.into(), ReadOnlyConfig::from_datadir(datadir))?;

    let executor_provider = EthExecutorProvider::ethereum(factory.chain_spec().clone());

    let start = 100;
    let end = 101;

    for block_number in start..end {
        // First get a provider for database reads
        let provider = factory.provider()?;

        // Then read the block we want to fetch
        let recovered_block = provider
            .recovered_block(block_number.into(), TransactionVariant::WithHash)?
            .ok_or(eyre::eyre!("block num not found"))?;

        // close the RO transaction
        drop(provider);

        // Get the state before the block in question
        let historical_state = factory.history_by_block_number(block_number - 1)?;

        let mut witness_record = ExecutionWitnessRecord::default();

        // Execute the block
        let state_db = StateProviderDatabase(&historical_state);
        let executor = executor_provider.batch_executor(state_db);

        let _output = executor
            .execute_with_state_closure(&recovered_block, |statedb: &State<_>| {
                witness_record.record_executed_state(statedb);
            })
            .map_err(|_| eyre::eyre!("could not execute transaction with state"))?;

        // Generate the ExecutionWitness
        let ExecutionWitnessRecord {
            hashed_state,
            codes,
            keys,
            lowest_block_number,
        } = witness_record;

        let state = historical_state.witness(Default::default(), hashed_state)?;
        let mut exec_witness = ExecutionWitness {
            state,
            codes,
            keys,
            headers: Default::default(),
        };

        let smallest = lowest_block_number.unwrap_or_else(|| {
            // Return only the parent header, if there were no calls to the
            // BLOCKHASH opcode.
            block_number.saturating_sub(1)
        });

        let range = smallest..block_number;

        exec_witness.headers = factory
            .headers_range(range)?
            .into_iter()
            .map(|header| {
                let mut serialized_header = Vec::new();
                header.encode(&mut serialized_header);
                serialized_header.into()
            })
            .collect();

        println!("{:?}", exec_witness);
    }

    Ok(())
}
