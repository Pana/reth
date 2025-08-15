mod state_tree_exporter;

use alloy_primitives::{Address, B256};
use reth_ethereum::{
    chainspec::{ChainSpecBuilder, HOLESKY},
    node::EthereumNode,
    primitives::{AlloyBlockHeader, SealedBlock, SealedHeader},
    provider::{
        providers::ReadOnlyConfig, AccountReader, BlockReader, BlockSource, HeaderProvider,
        ReceiptProvider, StateProvider, TransactionsProvider,
    },
    rpc::eth::primitives::Filter,
    TransactionSigned,
};
use crate::state_tree_exporter::StateTreeExporter;

fn main() -> eyre::Result<()> {
    let datadir = std::env::var("RETH_DATADIR")?;

    // Get the holesky spec
    let spec = ChainSpecBuilder::mainnet()
        .with_forks(HOLESKY.hardforks.clone())
        .chain(HOLESKY.chain)
        .genesis(HOLESKY.genesis.clone())
        .build();

    let factory = EthereumNode::provider_factory_builder()
        .open_read_only(spec.into(), ReadOnlyConfig::from_datadir(datadir))?;

    let provider = factory.provider()?;
    let state = factory.latest()?;

    let exporter = StateTreeExporter::new(provider, state);

    // println!("state: {:?}", state);

    // let accounts = exporter.export_all_accounts()?;
    // println!("accounts: {:?}", accounts.len());
    exporter.print_statistics()?;

    Ok(())
}

