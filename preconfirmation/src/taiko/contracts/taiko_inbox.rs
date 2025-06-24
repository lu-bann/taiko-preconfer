use alloy_sol_types::sol;

sol!(
    #[derive(Debug, Default)]
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

        struct TransitionState {
            bytes32 parentHash;
            bytes32 blockHash;
            bytes32 stateRoot;
            address prover;
            bool inProvingWindow;
            uint48 createdAt;
        }

        function getLastVerifiedTransition()
            external
            view
            returns (uint64 batchId_, uint64 blockId_, TransitionState memory ts_);

        function getLastSyncedTransition()
            external
            view
            returns (uint64 batchId_, uint64 blockId_, TransitionState memory ts_);

        event BatchProposed(BatchInfo info, BatchMetadata meta, bytes txList);
    }
);
