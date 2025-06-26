use alloy_sol_types::sol;

sol!(
    #[derive(Debug)]
    #[sol(rpc)]
     contract TaikoInbox {
        struct BaseFeeConfig {
            uint8 adjustmentQuotient;
            uint8 sharingPctg;
            uint32 gasIssuancePerSecond;
            uint64 minGasExcess;
            uint32 maxGasIssuancePerBlock;
        }

        struct BlockParams {
            uint16 numTransactions;
            uint8 timeShift;
            bytes32[] signalSlots;
        }

        struct BlobParams {
            bytes32[] blobHashes;
            uint8 firstBlobIndex;
            uint8 numBlobs;
            uint32 byteOffset;
            uint32 byteSize;
            uint64 createdIn;
        }

        struct BatchParams {
            address proposer;
            address coinbase;
            bytes32 parentMetaHash;
            uint64 anchorBlockId;
            uint64 lastBlockTimestamp;
            bool revertIfNotFirstProposal;
            BlobParams blobParams;
            BlockParams[] blocks;
        }

        struct BatchInfo {
            bytes32 txsHash;
            BlockParams[] blocks;
            bytes32[] blobHashes;
            bytes32 extraData;
            address coinbase;
            uint64 proposedIn;
            uint64 blobCreatedIn;
            uint32 blobByteOffset;
            uint32 blobByteSize;
            uint32 gasLimit;
            uint64 lastBlockId;
            uint64 lastBlockTimestamp;
            uint64 anchorBlockId;
            bytes32 anchorBlockHash;
            BaseFeeConfig baseFeeConfig;
        }

        struct BatchMetadata {
            bytes32 infoHash;
            address proposer;
            uint64 batchId;
            uint64 proposedAt;
        }

        struct Batch {
            bytes32 metaHash; // slot 1
            uint64 lastBlockId; // slot 2
            uint96 reserved3;
            uint96 livenessBond;
            uint64 batchId; // slot 3
            uint64 lastBlockTimestamp;
            uint64 anchorBlockId;
            uint24 nextTransitionId;
            uint8 reserved4;
            // The ID of the transaction that is used to verify this batch. However, if this batch is
            // not verified as the last one in a transaction, verifiedTransitionId will remain zero.
            uint24 verifiedTransitionId;
        }

        struct ForkHeights {
            uint64 ontake; // measured with block number.
            uint64 pacaya; // measured with the batch Id, not block number.
            uint64 shasta; // measured with the batch Id, not block number.
            uint64 unzen; // measured with the batch Id, not block number.
        }

        struct Config {
            /// @notice The chain ID of the network where Taiko contracts are deployed.
            uint64 chainId;
            /// @notice The maximum number of unverified batches the protocol supports.
            uint64 maxUnverifiedBatches;
            /// @notice Size of the batch ring buffer, allowing extra space for proposals.
            uint64 batchRingBufferSize;
            /// @notice The maximum number of verifications allowed when a batch is proposed or proved.
            uint64 maxBatchesToVerify;
            /// @notice The maximum gas limit allowed for a block.
            uint32 blockMaxGasLimit;
            /// @notice The amount of Taiko token as a prover liveness bond per batch.
            uint96 livenessBondBase;
            /// @notice The amount of Taiko token as a prover liveness bond per block. This field is
            /// deprecated and its value will be ignored.
            uint96 livenessBondPerBlock;
            /// @notice The number of batches between two L2-to-L1 state root sync.
            uint8 stateRootSyncInternal;
            /// @notice The max differences of the anchor height and the current block number.
            uint64 maxAnchorHeightOffset;
            /// @notice Base fee configuration
            BaseFeeConfig baseFeeConfig;
            /// @notice The proving window in seconds.
            uint16 provingWindow;
            /// @notice The time required for a transition to be used for verifying a batch.
            uint24 cooldownWindow;
            /// @notice The maximum number of signals to be received by TaikoL2.
            uint8 maxSignalsToReceive;
            /// @notice The maximum number of blocks per batch.
            uint16 maxBlocksPerBatch;
            /// @notice Historical heights of the forks.
            ForkHeights forkHeights;
        }

        struct TransitionState {
            bytes32 parentHash;
            bytes32 blockHash;
            bytes32 stateRoot;
            address prover;
            bool inProvingWindow;
            uint48 createdAt;
        }

        struct Stats1 {
            uint64 genesisHeight;
            uint64 __reserved2;
            uint64 lastSyncedBatchId;
            uint64 lastSyncedAt;
        }

        struct Stats2 {
            uint64 numBatches;
            uint64 lastVerifiedBatchId;
            bool paused;
            uint56 lastProposedIn;
            uint64 lastUnpausedAt;
        }

        struct State {
            bytes32 __reserve1; // slot 4 - was used as a ring buffer for Ether deposits
            Stats1 stats1; // slot 5
            Stats2 stats2; // slot 6
        }

        function getStats1() external view returns (Stats1 memory);

        function getStats2() external view returns (Stats2 memory);

        function state() external view returns (State memory);

        function pacayaConfig() external view returns (Config memory);

        function getBatch(uint64 _batchId) public view returns (Batch memory batch_);

        function bondToken() external view returns (address);

        function getLastVerifiedTransition()
            external
            view
            returns (uint64 batchId_, uint64 blockId_, TransitionState memory ts_);

        function getLastSyncedTransition()
            external
            view
            returns (uint64 batchId_, uint64 blockId_, TransitionState memory ts_);

        function getBatchVerifyingTransition(uint64 _batchId)
            public
            view
            returns (TransitionState memory ts_);

        event BatchProposed(BatchInfo info, BatchMetadata meta, bytes txList);
    }
);
