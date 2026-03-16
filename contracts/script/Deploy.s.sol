// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import "../src/YeetToken.sol";
import "../src/YeetPlatform.sol";
import "../src/YeetSwapVault.sol";

/**
 * Deploy sequence:
 *   1. YeetToken   (mints 21B, distributes allocations)
 *   2. YeetPlatform (holds reward pool)
 *   3. YeetSwapVault (holds swap reserve)
 *
 * Run on BSC Testnet:
 *   forge script script/Deploy.s.sol --rpc-url $BSC_TESTNET_RPC --broadcast --verify
 */
contract Deploy is Script {
    function run() external {
        uint256 pk = vm.envUint("DEPLOYER_PRIVATE_KEY");
        address deployer  = vm.addr(pk);
        address treasury  = vm.envAddress("TREASURY_ADDRESS");
        address team      = vm.envAddress("TEAM_ADDRESS");
        address liquidity = vm.envAddress("LIQUIDITY_ADDRESS");
        address community = vm.envAddress("COMMUNITY_ADDRESS");
        address oracle    = vm.envAddress("ORACLE_ADDRESS");

        vm.startBroadcast(pk);

        // 1. Token
        YeetToken token = new YeetToken(treasury, team, liquidity, community);
        console.log("YeetToken:", address(token));

        // 2. Platform
        YeetPlatform platform = new YeetPlatform(address(token));
        console.log("YeetPlatform:", address(platform));

        // 3. Swap Vault
        YeetSwapVault vault = new YeetSwapVault(address(token), oracle);
        console.log("YeetSwapVault:", address(vault));

        // 4. Wire up
        token.setRewardDistributor(address(platform));
        token.setSwapVault(address(vault));

        vm.stopBroadcast();

        console.log("\n=== YEET Token Deployment ===");
        console.log("Total supply  : 21,000,000,000 YEET");
        console.log("Burn address  : 0x000000000000000000000000000000000000dEaD");
        console.log("Swap reserve  : 10,500,000,000 YEET (50%)");
        console.log("Reward pool   :  4,200,000,000 YEET (20%)");
        console.log("Treasury      :  2,100,000,000 YEET (10%)");
        console.log("Team vesting  :  1,680,000,000 YEET  (8%)");
        console.log("Liquidity     :  1,260,000,000 YEET  (6%)");
        console.log("Community     :  1,260,000,000 YEET  (6%)");
    }
}