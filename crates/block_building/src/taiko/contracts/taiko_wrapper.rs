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
    struct BlobParams {
        // The hashes of the blob. Note that if this array is not empty.  `firstBlobIndex` and
        // `numBlobs` must be 0.
        bytes32[] blobHashes;
        // The index of the first blob in this batch.
        uint8 firstBlobIndex;
        // The number of blobs in this batch. Blobs are initially concatenated and subsequently
        // decompressed via Zlib.
        uint8 numBlobs;
        // The byte offset of the blob in the batch.
        uint32 byteOffset;
        // The byte size of the blob.
        uint32 byteSize;
        // The block number when the blob was created. This value is only non-zero when
        // `blobHashes` are non-empty.
        uint64 createdIn;
    }

    #[derive(Debug)]
    struct BatchParams {
        address proposer;
        address coinbase;
        bytes32 parentMetaHash;
        uint64 anchorBlockId;
        uint64 lastBlockTimestamp;
        bool revertIfNotFirstProposal;
        // Specifies the number of blocks to be generated from this batch.
        BlobParams blobParams;
        BlockParams[] blocks;
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
