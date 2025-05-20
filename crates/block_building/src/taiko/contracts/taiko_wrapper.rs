use alloy_sol_types::sol;

sol!(
    #[derive(Debug)]
    struct BaseFeeConfig {
        uint8 adjustmentQuotient;
        uint8 sharingPctg;
        uint32 gasIssuancePerSecond;
        uint64 minGasExcess;
        uint32 maxGasIssuancePerBlock;
    }
    #[derive(Debug)]
    struct BlockParams {
        uint16 numTransactions;
        uint8 timeShift;
        bytes32[] signalSlots;
    }
    #[derive(Debug)]
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

    #[derive(Debug)]
    struct BatchMetadata {
        bytes32 infoHash;
        address proposer;
        uint64 batchId;
        uint64 proposedAt;
    }

     #[sol(rpc)]
     contract TaikoWrapper {
        #[derive(Debug)]
        function proposeBatch(
            bytes calldata _params,
            bytes calldata _txList
        )
        external
        returns (BatchInfo memory, BatchMetadata memory);
    }
);
