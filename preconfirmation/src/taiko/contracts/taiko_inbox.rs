use alloy_sol_types::sol;

sol!(
     #[sol(rpc)]
     contract TaikoInbox {
        #[derive(Debug)]
        struct TransitionState {
            bytes32 parentHash;
            bytes32 blockHash;
            bytes32 stateRoot;
            address prover;
            bool inProvingWindow;
            uint48 createdAt;
        }

        #[derive(Debug)]
        function getLastVerifiedTransition()
            external
            view
            returns (uint64 batchId_, uint64 blockId_, TransitionState memory ts_);

        #[derive(Debug)]
        function getLastSyncedTransition()
            external
            view
            returns (uint64 batchId_, uint64 blockId_, TransitionState memory ts_);
    }
);
