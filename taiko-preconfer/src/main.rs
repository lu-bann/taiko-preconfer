use alloy_primitives::Address;
use alloy_provider::{Provider, ProviderBuilder, WsConnect};
use alloy_rpc_types::Header;
use alloy_rpc_types_engine::JwtSecret;
use futures::{FutureExt, future::BoxFuture};
use std::{pin::Pin, time::Duration};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{join, sync::Mutex, time::sleep};
use tracing::{error, info};

use block_building::{
    active_operator_model::ActiveOperatorModel,
    preconf::{Preconfer, config::Config},
    rpc_client::RpcClient,
    slot_model::HOLESKY_SLOT_MODEL,
    taiko::{
        contracts::TaikoAnchorInstance,
        hekla::{CHAIN_ID, addresses::get_taiko_anchor_address, get_basefee_config_v2},
        taiko_client::{ITaikoClient, TaikoClient},
        taiko_l1_client::{ITaikoL1Client, TaikoL1Client},
    },
    time_provider::{ITimeProvider, SystemTimeProvider},
};

mod rpc;
use crate::rpc::{get_auth_client, get_client};

mod error;
use crate::error::{ApplicationError, ApplicationResult};

async fn stream_block_headers_with_builder<
    'a,
    L1Client: ITaikoL1Client,
    Taiko: ITaikoClient,
    TimeProvider: ITimeProvider,
    T: Fn(
            Header,
            Arc<Mutex<Preconfer<L1Client, Taiko, TimeProvider>>>,
            Arc<Mutex<ActiveOperatorModel>>,
            Duration,
        ) -> BoxFuture<'a, ApplicationResult<()>>
        + Send,
>(
    url: &str,
    f: T,
    block_builder: Arc<Mutex<Preconfer<L1Client, Taiko, TimeProvider>>>,
    active_operator_model: Arc<Mutex<ActiveOperatorModel>>,
    l2_block_time: Duration,
) -> ApplicationResult<()> {
    info!("Subscribe to headers at {url}");
    let ws = WsConnect::new(url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let mut stream = provider.subscribe_blocks().await?;

    while let Ok(header) = stream.recv().await {
        f(
            header,
            block_builder.clone(),
            active_operator_model.clone(),
            l2_block_time,
        )
        .await?;
    }
    Err(ApplicationError::WsConnectionLost {
        url: url.to_string(),
    })
}

async fn stream_block_headers_into<
    'a,
    T: Fn(Header, Arc<Mutex<u64>>) -> BoxFuture<'a, ApplicationResult<()>>,
>(
    url: &str,
    f: T,
    current: Arc<Mutex<u64>>,
) -> ApplicationResult<()> {
    info!("Subscribe to headers at {url}");
    let ws = WsConnect::new(url);
    let provider = ProviderBuilder::new().connect_ws(ws).await?;
    let mut stream = provider.subscribe_blocks().await?;

    while let Ok(header) = stream.recv().await {
        f(header, current.clone()).await?;
    }
    Err(ApplicationError::WsConnectionLost {
        url: url.to_string(),
    })
}

async fn wait_until_next_block(
    now: SystemTime,
    current_block_time: Duration,
    l2_block_time: Duration,
) -> Result<(), std::time::SystemTimeError> {
    let desired_next_block_time = UNIX_EPOCH
        .checked_add(current_block_time)
        .map(|x| x.checked_add(l2_block_time).unwrap_or(UNIX_EPOCH))
        .unwrap_or(UNIX_EPOCH);
    if let Ok(remaining) = desired_next_block_time.duration_since(now) {
        sleep(remaining).await;
    }
    Ok(())
}

async fn process_l2_header(
    header: Header,
    block_builder: Arc<Mutex<Preconfer<TaikoL1Client, TaikoClient, SystemTimeProvider>>>,
    active_operator_model: Arc<Mutex<ActiveOperatorModel>>,
    l2_block_time: Duration,
) -> ApplicationResult<()> {
    let num = header.number;
    let hash = header.hash;

    info!("L2 🗣 #{:<10} {}", num, hash);
    let now = SystemTime::now();
    if let Err(err) =
        wait_until_next_block(now, Duration::from_secs(header.timestamp), l2_block_time).await
    {
        error!("Error during wait for block building start: {:?}", err)
    }
    let slot = HOLESKY_SLOT_MODEL.get_slot(header.timestamp);
    // Set active operator model to be always active
    let epoch = if active_operator_model
        .lock()
        .await
        .within_handover_period(slot.slot)
    {
        slot.epoch + 1
    } else {
        slot.epoch
    };
    info!("Set active epoch to {} for slot {:?}", epoch, slot);
    active_operator_model
        .lock()
        .await
        .set_next_active_epoch(epoch);
    if active_operator_model.lock().await.can_preconfirm(slot) {
        if let Err(err) = block_builder.lock().await.build_block(header).await {
            error!("Error during block building: {:?}", err)
        }
    } else {
        info!("Not active operator. Skip block building.");
    }
    Ok(())
}

async fn run_preconfer() -> ApplicationResult<()> {
    dotenv::dotenv()?;

    let config = Config::try_from_env()?;

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    let l2_client = RpcClient::new(get_client(&config.l2_client_url)?);
    let jwt_secret =
        JwtSecret::from_hex("654c8ed1da58823433eb6285234435ed52418fa9141548bca1403cc0ad519432")
            .unwrap();
    let auth_client = RpcClient::new(get_auth_client(&config.l2_auth_client_url, jwt_secret)?);
    let l1_provider = ProviderBuilder::new()
        .connect(&config.l1_client_url)
        .await?;
    let l1_client = TaikoL1Client::new(
        RpcClient::new(get_client(&config.l1_client_url)?),
        l1_provider,
    );

    let taiko_anchor_address = get_taiko_anchor_address();
    let provider = ProviderBuilder::new()
        .connect(&config.l2_client_url)
        .await?;
    let taiko_anchor = TaikoAnchorInstance::new(taiko_anchor_address, provider.clone());

    let taiko = TaikoClient::new(
        l2_client,
        auth_client,
        taiko_anchor,
        provider,
        get_basefee_config_v2(),
        CHAIN_ID,
    );
    let l2_block_time = config.l2_block_time;
    let handover_slots = config.handover_window_slots as u64;
    let block_builder = Arc::new(Mutex::new(Preconfer::new(
        config.anchor_id_lag,
        l1_client,
        taiko,
        Address::random(),
        SystemTimeProvider::new(),
    )));
    let shared_last_l1_block_number = block_builder.lock().await.shared_last_l1_block_number();

    let process_l1_header = {
        |header: Header, current: Arc<Mutex<u64>>| {
            async move {
                let num = header.number;
                let hash = header.hash;

                info!("L1 🗣 #{:<10} {}", num, hash);
                info!("{:?}", HOLESKY_SLOT_MODEL.get_slot(header.timestamp));
                *current.lock().await = header.number;
                Ok(())
            }
            .boxed()
        }
    };

    let slots_per_epoch = 32;
    let active_operator_model = Arc::new(Mutex::new(ActiveOperatorModel::new(
        handover_slots,
        slots_per_epoch,
    )));
    let process_l2_header2 =
        |header: Header,
         block_builder: Arc<Mutex<Preconfer<TaikoL1Client, TaikoClient, SystemTimeProvider>>>,
         active_operator_model: Arc<Mutex<ActiveOperatorModel>>,
         l2_block_time: Duration|
         -> Pin<Box<dyn Future<Output = ApplicationResult<()>> + Send>> {
            Box::pin(process_l2_header(
                header,
                block_builder,
                active_operator_model,
                l2_block_time,
            ))
        };

    let _ = join!(
        stream_block_headers_into(
            &config.l1_ws_url,
            process_l1_header,
            shared_last_l1_block_number.clone()
        ),
        stream_block_headers_with_builder(
            &config.l2_ws_url,
            process_l2_header2,
            block_builder,
            active_operator_model,
            l2_block_time
        ),
    );

    Ok(())
}

#[tokio::main]
async fn main() -> ApplicationResult<()> {
    run_preconfer().await
}
