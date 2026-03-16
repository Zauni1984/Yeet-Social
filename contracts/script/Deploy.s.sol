// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Script.sol";
import "../src/YeetToken.sol";
import "../src/YeetTipping.sol";
import "../src/YeetNFT.sol";

/// @notice Deploy all Yeet contracts to BSC Chapel Testnet
/// @dev Run: forge script script/Deploy.s.sol --rpc-url chapel --broadcast --verify
contract Deploy is Script {
    function run() external {
        uint256 deployerKey = vm.envUint("PRIVATE_KEY");
        address deployer    = vm.addr(deployerKey);
        address rewardPool  = vm.envOr("REWARD_POOL", deployer); // fallback to deployer on testnet

        console.log("Deploying from:", deployer);
        console.log("Network:        BSC Chapel Testnet (97)");
        console.log("Reward pool:   ", rewardPool);

        vm.startBroadcast(deployerKey);

        // 1. Deploy YEET token
        YeetToken token = new YeetToken(deployer, rewardPool);
        console.log("YeetToken:    ", address(token));

        // 2. Deploy Tipping (platform fee wallet = deployer on testnet)
        YeetTipping tipping = new YeetTipping(address(token), deployer, deployer);
        console.log("YeetTipping:  ", address(tipping));

        // 3. Deploy NFT
        YeetNFT nft = new YeetNFT(deployer);
        console.log("YeetNFT:      ", address(nft));

        vm.stopBroadcast();

        // Print summary
        console.log("\n=== Deployment Summary ===");
        console.log("YEET Token:  ", address(token));
        console.log("Tipping:     ", address(tipping));
        console.log("NFT:         ", address(nft));
        console.log("\nVerify on BSCScan Testnet:");
        console.log("https://testnet.bscscan.com/address/", address(token));
        console.log("https://testnet.bscscan.com/address/", address(tipping));
        console.log("https://testnet.bscscan.com/address/", address(nft));
    }
}