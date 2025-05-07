use alloy_sol_types::sol;

sol!(
     #[sol(rpc)]
     contract TaikoAnchor {
        #[derive(Debug)]
        struct BaseFeeConfig {
            uint8 adjustmentQuotient;
            uint8 sharingPctg;
            uint32 gasIssuancePerSecond;
            uint64 minGasExcess;
            uint32 maxGasIssuancePerBlock;
        }

        #[derive(Debug)]
        function getBasefeeV2(
            uint32 _parentGasUsed,
            uint64 _blockTimestamp,
            BaseFeeConfig calldata _baseFeeConfig
        )
        public
        view
        returns (uint256 basefee_, uint64 newGasTarget_, uint64 newGasExcess_);

        #[derive(Debug)]
        function anchorV3(
            uint64 _anchorBlockId,
            bytes32 _anchorStateRoot,
            uint32 _parentGasUsed,
            BaseFeeConfig calldata _baseFeeConfig,
            bytes32[] calldata _signalSlots
        );
    }
);
