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
            uint64 _anchorBlockId,                      // l1_block number on l1
            bytes32 _anchorStateRoot,                   // l1_block.header.state_root for _anchorBlockId
            uint32 _parentGasUsed,                      // l2_block.header.gas_used from previous l2 block
            BaseFeeConfig calldata _baseFeeConfig,      // hard-coded -> later from l1
            bytes32[] calldata _signalSlots             // hard-coded empty, may need fix
        );

        #[derive(Debug)]
        function lastSyncedBlock() public view returns (uint64);
    }
);
