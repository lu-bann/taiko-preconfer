use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use alloy_primitives::Address;
use futures::{Stream, StreamExt, pin_mut};
use preconfirmation::{
    preconf::{
        BlockBuilder,
        sequencing_monitor::{TaikoSequencingMonitor, TaikoStatusMonitor},
        slot_model::SlotModel as PreconfirmationSlotModel,
    },
    slot::SubSlot,
    slot_model::{HOLESKY_GENESIS_TIMESTAMP, SlotModel},
    taiko::{contracts::TaikoWhitelistInstance, taiko_l2_client::ITaikoL2Client},
    time_provider::ITimeProvider,
    util::{log_error, now_as_millis, now_as_secs},
};
use tracing::{debug, info, instrument};

use crate::{
    error::ApplicationResult,
    util::{set_active_operator_for_next_period, set_active_operator_if_necessary},
};

#[instrument(name = "📋", skip_all)]
#[allow(clippy::too_many_arguments)]
pub async fn run<L2Client: ITaikoL2Client, TimeProvider: ITimeProvider>(
    stream: impl Stream<Item = SubSlot>,
    builder: BlockBuilder<L2Client, TimeProvider>,
    preconfirmation_slot_model: PreconfirmationSlotModel,
    whitelist: TaikoWhitelistInstance,
    sequencing_monitor: TaikoSequencingMonitor<TaikoStatusMonitor>,
    handover_timeout: Duration,
    subslots_per_slot: u64,
    waiting_for_previous_preconfer: Arc<AtomicBool>,
    polling_period: Duration,
    status_sync_max_delay: Duration,
) -> ApplicationResult<()> {
    let mut preconfirmation_slot_model = preconfirmation_slot_model;
    pin_mut!(stream);
    let preconfer_address = builder.address();
    let mut current_epoch_preconfer = Address::ZERO;
    let mut next_epoch_preconfer = Address::ZERO;

    let slot_model = SlotModel::holesky();
    let current_slot = slot_model.get_slot(now_as_secs());
    if let Some(current_preconfer) = log_error(
        whitelist.getOperatorForCurrentEpoch().call().await,
        "Failed to read current preconfer",
    ) {
        set_active_operator_if_necessary(
            &current_preconfer,
            &preconfer_address,
            &mut preconfirmation_slot_model,
            &current_slot,
        );
        current_epoch_preconfer = current_preconfer;
    }
    if let Some(next_preconfer) = log_error(
        whitelist.getOperatorForNextEpoch().call().await,
        "Failed to read preconfer for next epoch",
    ) {
        set_active_operator_for_next_period(
            &next_preconfer,
            &preconfer_address,
            &mut preconfirmation_slot_model,
            &current_slot,
        );
        next_epoch_preconfer = next_preconfer;
    }

    info!("Current preconfer: {current_epoch_preconfer}, next preconfer: {next_epoch_preconfer}");
    loop {
        if let Some(subslot) = stream.next().await {
            info!("📩 Received subslot: {:?}", subslot);
            if waiting_for_previous_preconfer.load(Ordering::Relaxed) {
                info!("Waiting for previous preconfer to finish.");
                continue;
            }
            let slot_timestamp =
                HOLESKY_GENESIS_TIMESTAMP + subslot.slot.epoch * 32 * 12 + subslot.sub_slot * 6;
            info!(
                "slot number: L1={}, L2={}, L2 time={}, now={}",
                subslot.slot.epoch * 32 + subslot.slot.slot,
                subslot.slot.epoch * 64 + subslot.sub_slot,
                slot_timestamp,
                now_as_secs(),
            );
            if subslot.sub_slot == 0 {
                current_epoch_preconfer = next_epoch_preconfer;
                next_epoch_preconfer = Address::ZERO;
            }

            debug!(
                "Current preconfer: {current_epoch_preconfer}, next preconfer: {next_epoch_preconfer}"
            );

            let is_last_slot_before_handover_window =
                preconfirmation_slot_model.is_last_slot_before_handover_window(subslot.slot.slot);
            if preconfirmation_slot_model.can_preconfirm(&subslot.slot)
                || (current_epoch_preconfer == preconfer_address
                    && next_epoch_preconfer == preconfer_address)
            {
                if preconfirmation_slot_model.is_first_preconfirmation_slot(&subslot.slot)
                    && subslot.sub_slot == 0
                {
                    let handover_timeout = handover_timeout;
                    let sequencing_monitor = sequencing_monitor.clone();
                    waiting_for_previous_preconfer.store(true, Ordering::Relaxed);
                    let waiting_for_previous_preconfer_spawn =
                        waiting_for_previous_preconfer.clone();
                    tokio::spawn(async move {
                        if log_error(
                            tokio::time::timeout(handover_timeout, sequencing_monitor.ready())
                                .await,
                            "❌ Out of sync after handover period",
                        )
                        .is_some()
                        {
                            info!("✅ In sync after handover period");
                        }
                        waiting_for_previous_preconfer_spawn.store(false, Ordering::Relaxed);
                    });
                }
                if waiting_for_previous_preconfer.load(Ordering::Relaxed) {
                    let slot_timestamp_ms = slot_timestamp as u128 * 1000;
                    let mut skip_slot = true;
                    while now_as_millis() < slot_timestamp_ms + status_sync_max_delay.as_millis() {
                        skip_slot = waiting_for_previous_preconfer.load(Ordering::Relaxed);
                        tokio::time::sleep(polling_period).await;
                    }
                    if skip_slot {
                        continue;
                    }
                }

                let end_of_sequencing = is_last_slot_before_handover_window
                    && subslot.sub_slot % subslots_per_slot == subslots_per_slot - 1;
                log_error(
                    tokio::time::timeout(
                        Duration::from_millis(1500),
                        builder.build_block(slot_timestamp, end_of_sequencing),
                    )
                    .await,
                    "Error building block",
                );
            } else {
                info!("Not active operator. Skip block building.");
            }

            if is_last_slot_before_handover_window {
                if let Some(next_preconfer) = log_error(
                    whitelist.getOperatorForNextEpoch().call().await,
                    "Failed to read preconfer for next epoch",
                ) {
                    set_active_operator_for_next_period(
                        &next_preconfer,
                        &preconfer_address,
                        &mut preconfirmation_slot_model,
                        &subslot.slot,
                    );
                    next_epoch_preconfer = next_preconfer;
                }
            }
        }
    }
}
