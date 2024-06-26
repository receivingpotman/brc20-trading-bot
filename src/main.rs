mod db;
mod platform;
mod robot;
mod types;
mod utils;

use crate::db::Storage;
use crate::types::{FraAccount, ListResponse, Rpc};
use anyhow::Result;
use clap::Parser;
use dotenv::dotenv;
use env_logger::Target;
use log::info;
use serde_json::from_str;
use sqlx::pool::PoolOptions;
use sqlx::{PgPool, Pool, Postgres};
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;
use std::{env, io};
use std::{fs::File, io::Write};
use tokio::time::interval;
use tokio::{runtime, time};
use utils::gen_accounts;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value_t = 10)]
    accounts: i32,
}

const ACCOUNT_MINT: &'static str = "accounts-mint.txt";
const ACCOUNT_BUY: &'static str = "accounts-buy.txt";
const MINT_LIMIT: usize = 7;
const ACCOUNT_TYPE_MINT: i32 = 1;
const ACCOUNT_TYPE_BUY: i32 = 2;

#[derive(Debug)]
struct BotServer {
    storage: Arc<Storage>,
    accounts_mint: Vec<FraAccount>,
    accounts_buy: Vec<FraAccount>,
    rpc: Arc<Rpc>,
}

impl BotServer {
    pub fn new(
        pool: PgPool,
        rpc: Rpc,
        accounts_mint: Vec<FraAccount>,
        accounts_buy: Vec<FraAccount>,
    ) -> Result<Self> {
        Ok(Self {
            storage: Arc::new(Storage::new(pool)),
            accounts_mint,
            accounts_buy,
            rpc: Arc::new(rpc),
        })
    }

    pub async fn prepare_accounts(&self) -> Result<()> {
        self.storage
            .insert_accounts(ACCOUNT_TYPE_MINT, &self.accounts_mint)
            .await?;

        self.storage
            .insert_accounts(ACCOUNT_TYPE_BUY, &self.accounts_buy)
            .await?;

        Ok(())
    }

    pub async fn get_token_list(
        &self,
        token: &str,
        page: i32,
        page_size: i32,
    ) -> Result<ListResponse> {
        let res = self.rpc.get_token_list(token, page, page_size).await?;
        Ok(res)
    }

    pub async fn get_owned_utxos(&self) {}
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    env_logger::builder().target(Target::Stdout).init();

    let db_url = env::var("DATABASE_URL")?;
    let pool: Pool<Postgres> = PoolOptions::new()
        .connect(&db_url)
        .await
        .expect("connect DB");
    println!("Connecting DB...ok");

    let args = Args::parse();
    let accounts_mint: Vec<FraAccount> = match File::open(ACCOUNT_MINT) {
        Ok(mut f) => {
            let mut contents = String::new();
            f.read_to_string(&mut contents)?;
            let accounts = serde_json::from_str(&contents)?;
            println!("Reading accounts-mint... ok");
            accounts
        }
        Err(e) => {
            if e.kind() == io::ErrorKind::NotFound {
                let accounts = gen_accounts(args.accounts)?;
                let mut f = File::create(ACCOUNT_MINT)?;
                let s = serde_json::to_string_pretty(&accounts)?;
                let _ = f.write_all(s.as_bytes())?;
                println!("Generating accounts-mint... ok");
                accounts
            } else {
                panic!("{}", e);
            }
        }
    };

    let accounts_buy: Vec<FraAccount> = match File::open(ACCOUNT_BUY) {
        Ok(mut f) => {
            let mut contents = String::new();
            f.read_to_string(&mut contents)?;
            let accounts = serde_json::from_str(&contents)?;
            println!("Reading accounts-buy... ok");
            accounts
        }
        Err(e) => {
            if e.kind() == io::ErrorKind::NotFound {
                let accounts = gen_accounts(args.accounts)?;
                let mut f = File::create(ACCOUNT_BUY)?;
                let s = serde_json::to_string_pretty(&accounts)?;
                let _ = f.write_all(s.as_bytes())?;
                println!("Generating accounts-buy... ok");
                accounts
            } else {
                panic!("{}", e);
            }
        }
    };
    let token = env::var("TOKEN")?;
    let ex_rpc_url = env::var("EX_RPC")?;
    let node_rpc_url = env::var("NODE_RPC")?;
    let node_api_port = env::var("NODE_API_PORT")?;
    let list_sum_amount = from_str::<u64>(&env::var("LIST_SUM_AMOUNT")?)?;

    let rpc = Rpc::new(&ex_rpc_url, &format!("{}:{}", node_rpc_url, node_api_port))?;

    let floor_prices: Vec<u64> = vec![
        123000000, 250000000, 450000000, 200000000, 220000000, 300000000,
    ];
    let mut price_index = 1;
    let mut account_index = 0;

    let server = BotServer::new(pool, rpc, accounts_mint, accounts_buy)?;
    server.prepare_accounts().await?;

    let mut timer1 = time::interval(time::Duration::from_secs(5));
    let mut timer2 = time::interval(time::Duration::from_secs(10));

    loop {
        tokio::select! {
            _ = timer1.tick() => {
                let list_res = server.get_token_list(&token, 1, 50).await?;
                if list_res.total == 0 {
                    println!("[List] no lists");
                    continue;
                }
                let mut sum = 0;
                for item in list_res.data.unwrap() {
                    sum += from_str::<u64>(&item.amount)?;
                }
                let pages = list_res.total / 50 + 1;
                for page in 1..pages {
                    let list_res = server.get_token_list(&token, page, 50).await?;
                    if list_res.total == 0 {
                        continue;
                    }
                    for item in list_res.data.unwrap() {
                        sum += from_str::<u64>(&item.amount)?;
                    }
                }

                if sum >= list_sum_amount{
                    continue;
                }
                println!("[List] add lists");
                todo!()
            },
            _ = timer2.tick() => {
                let cur_floor_price = floor_prices[price_index%floor_prices.len()];

                let list_res = server.get_token_list(&token, 1, 50).await?;
                if list_res.total == 0 {
                    println!("[buy] no lists");
                    continue;
                }
                let pages = list_res.total / 50 + 1;
                if let Some(items) = list_res.data {
                    for i in 0..items.len() {
                        let price = from_str::<u64>(&items[i].price)?;
                        if price <= cur_floor_price {
                            todo!()
                        }
                    }
                }
                for page in 1..pages {
                    let list_res = server.get_token_list(&token, page, 50).await?;
                    if list_res.total == 0 {
                        println!("[List] no lists");
                        continue;
                    }

                    println!("total lists: {}", list_res.total);
                    if let Some(items) = list_res.data {
                        for i in 0..items.len() {
                            todo!()
                        }
                    }
                }

                price_index += 1;
            }
        }
    }

    Ok(())
}
