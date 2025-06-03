use alloy_sol_types::sol;

sol!(
     #[sol(rpc)]
     contract PreconfWhitelist {
        #[derive(Debug)]
        function getOperatorForCurrentEpoch() external view returns (address);

        #[derive(Debug)]
        function getOperatorForNextEpoch() external view returns (address);
    }
);
