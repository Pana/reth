use alloy_primitives::{Address, B256, U256};
use reth_ethereum::{
    chainspec::ChainSpecBuilder,
    evm::revm::primitives::Bytes,
    node::EthereumNode,
    primitives::{Account, Bytecode},
    provider::{
        db::{
            cursor::{DbCursorRO, DbDupCursorRO},
            tables, 
            transaction::DbTx,
        },
        providers::ReadOnlyConfig,
        ProviderResult, ProviderError,
    },
    storage::{DBProvider, StateProvider},
};
use std::collections::HashMap;

/// 表示一个完整的账户状态信息
#[derive(Debug, Clone)]
pub struct AccountStateInfo {
    /// 账户地址
    pub address: Address,
    /// 账户基本信息 (balance, nonce, code_hash)
    pub account: Account,
    /// 合约字节码 (如果是合约的话)
    pub bytecode: Option<Bytecode>,
    /// 所有存储槽位
    pub storage: HashMap<B256, U256>,
}

/// 状态树导出器
pub struct StateTreeExporter<P: DBProvider> {
    provider: P,
    state_provider: Box<dyn StateProvider>,
}

impl<P: DBProvider> StateTreeExporter<P> {
    /// 创建新的状态树导出器
    pub fn new(provider: P, state_provider: Box<dyn StateProvider>) -> Self {
        Self { provider, state_provider }
    }

    /// 遍历所有账户并导出状态信息
    pub fn export_all_accounts(&self) -> ProviderResult<Vec<AccountStateInfo>> {
        let mut accounts = Vec::new();
        
        // 使用数据库游标遍历所有账户
        let mut accounts_cursor = self.provider.tx_ref().cursor_read::<tables::PlainAccountState>()?;
        
        // 从第一个账户开始遍历
        if let Some((address, account)) = accounts_cursor.first()? {
            // 处理第一个账户
            if let Some(account_info) = self.get_account_state_info(address, account)? {
                accounts.push(account_info);
            }
            
            // 继续遍历剩余的账户
            while let Some((address, account)) = accounts_cursor.next()? {
                if let Some(account_info) = self.get_account_state_info(address, account)? {
                    accounts.push(account_info);
                }
            }
        }
        
        Ok(accounts)
    }

    /// 获取单个账户的完整状态信息
    fn get_account_state_info(
        &self,
        address: Address,
        account: Account,
    ) -> ProviderResult<Option<AccountStateInfo>> {
        // 获取合约字节码
        let bytecode = self.state_provider.account_code(&address)?;
        
        // 获取存储信息
        let storage = self.get_account_storage(address)?;
        
        Ok(Some(AccountStateInfo {
            address,
            account,
            bytecode,
            storage,
        }))
    }

    /// 获取账户的所有存储槽位
    fn get_account_storage(&self, address: Address) -> ProviderResult<HashMap<B256, U256>> {
        let mut storage = HashMap::new();
        let mut storage_cursor = self.provider.tx_ref().cursor_dup_read::<tables::PlainStorageState>()?;
        
        // 查找该账户的第一个存储条目
        if let Some((_, first_entry)) = storage_cursor.seek_exact(address)? {
            storage.insert(first_entry.key, first_entry.value);
            
            // 继续获取该账户的所有存储条目
            while let Some((_, entry)) = storage_cursor.next_dup()? {
                storage.insert(entry.key, entry.value);
            }
        }
        
        Ok(storage)
    }

    /// 导出状态信息到JSON文件
    pub fn export_to_json(&self, file_path: &str) -> ProviderResult<()> {
        let accounts = self.export_all_accounts()?;
        
        // 转换为可序列化的格式
        let export_data: Vec<serde_json::Value> = accounts
            .into_iter()
            .map(|account_info| {
                serde_json::json!({
                    "address": format!("0x{:x}", account_info.address),
                    "balance": format!("0x{:x}", account_info.account.balance),
                    "nonce": account_info.account.nonce,
                    "code_hash": account_info.account.bytecode_hash.map(|hash| format!("0x{:x}", hash)),
                    "bytecode": account_info.bytecode.map(|code| format!("0x{}", hex::encode(code.0.bytecode()))),
                    "storage": account_info.storage.into_iter().map(|(key, value)| {
                        (format!("0x{:x}", key), format!("0x{:x}", value))
                    }).collect::<HashMap<String, String>>(),
                })
            })
            .collect();
        
        // 写入JSON文件
        let json_content = serde_json::to_string_pretty(&export_data).map_err(|e| ProviderError::other(e))?;
        std::fs::write(file_path, json_content).map_err(|e| ProviderError::other(e))?;
        
        println!("成功导出 {} 个账户的状态信息到 {}", export_data.len(), file_path);
        Ok(())
    }

    /// 导出状态信息到CSV文件
    pub fn export_to_csv(&self, file_path: &str) -> ProviderResult<()> {
        let accounts = self.export_all_accounts()?;
        
        let mut csv_writer = csv::Writer::from_path(file_path).map_err(|e| ProviderError::other(e))?;
        
        // 写入CSV头部
        csv_writer.write_record(&[
            "address",
            "balance",
            "nonce", 
            "code_hash",
            "has_bytecode",
            "storage_count"
        ])
        .map_err(|e| ProviderError::other(e))?;
        
        // 写入数据行
        for account_info in &accounts {
            csv_writer.write_record(&[
                format!("0x{:x}", account_info.address),
                format!("0x{:x}", account_info.account.balance),
                account_info.account.nonce.to_string(),
                account_info.account.bytecode_hash
                    .map(|hash| format!("0x{:x}", hash))
                    .unwrap_or_default(),
                account_info.bytecode.is_some().to_string(),
                account_info.storage.len().to_string(),
            ])
            .map_err(|e| ProviderError::other(e))?;
        }
        
        csv_writer.flush().map_err(|e| ProviderError::other(e))?;
        println!("成功导出 {} 个账户的状态信息到 {}", accounts.len(), file_path);
        Ok(())
    }

    /// 打印统计信息
    pub fn print_statistics(&self) -> ProviderResult<()> {
        let accounts = self.export_all_accounts()?;
        
        let total_balance: U256 = accounts.iter().map(|acc| acc.account.balance).sum();
        let contract_count = accounts.iter().filter(|acc| acc.bytecode.is_some()).count();
        let total_storage_slots: usize = accounts.iter().map(|acc| acc.storage.len()).sum();
        
        println!("=== 状态树统计信息 ===");
        println!("总账户数: {}", accounts.len());
        println!("合约账户数: {}", contract_count);
        println!("普通账户数: {}", accounts.len() - contract_count);
        println!("总余额: 0x{:x}", total_balance);
        println!("总存储槽位数: {}", total_storage_slots);
        
        // 显示余额最高的前10个账户
        let mut sorted_accounts: Vec<_> = accounts.into_iter().collect();
        sorted_accounts.sort_by(|a, b| b.account.balance.cmp(&a.account.balance));
        
        println!("\n=== 余额最高的前10个账户 ===");
        for (i, account) in sorted_accounts.iter().take(10).enumerate() {
            println!("{}. 地址: 0x{:x}, 余额: 0x{:x}", 
                i + 1, account.address, account.account.balance);
        }
        
        Ok(())
    }
}

/// 从reth数据库创建状态树导出器的便捷函数
pub fn create_state_tree_exporter(datadir: &str) -> eyre::Result<StateTreeExporter<impl DBProvider>> {
    let spec = ChainSpecBuilder::mainnet().build();
    let factory = EthereumNode::provider_factory_builder()
        .open_read_only(spec.into(), ReadOnlyConfig::from_datadir(datadir.to_string()))?;

    let provider = factory.provider()?;
    let state_provider = factory.latest()?;
    
    Ok(StateTreeExporter::new(provider, state_provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_state_info_creation() {
        let address = Address::random();
        let account = Account {
            balance: U256::from(1000),
            nonce: 5,
            bytecode_hash: Some(B256::random()),
        };
        
        // 这里只是测试结构体创建，实际使用需要真实的数据库连接
        let _account_info = AccountStateInfo {
            address,
            account,
            bytecode: None,
            storage: HashMap::new(),
        };
    }
}

