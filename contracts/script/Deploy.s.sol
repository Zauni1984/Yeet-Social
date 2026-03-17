// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Script.sol";
import "../src/YeetToken.sol";
import "../src/YeetTipping.sol";
import "../src/YeetNFT.sol";

/// @notice Deploy all Yeet contracts
/// @dev Testnet:  forge script script/Deploy.s.sol --rpc-url bsc_testnet --broadcast --verify -vvvv
/// @dev Mainnet:  forge script script/Deploy.s.sol --rpc-url bsc_mainnet --broadcast --verify -vvvv
contract Deploy is Script {
    function run() external {
        uint256 deployerKey = vm.envUint("PRIVATE_KEY");
        address deployer    = vm.addr(deployerKey);
        address rewardPool  = vm.envOr("REWARD_POOL", deployer);

        console.log("Deployer:    ", deployer);
        console.log("Chain ID:    ", block.chainid);
        console.log("Network:     ", block.chainid == 97  ? "BNB Smart Chain Testnet"
                                   : block.chainid == 56  ? "BNB Smart Chain Mainnet"
                                   : "Unknown");
        console.log("Reward pool: ", rewardPool);

        vm.startBroadcast(deployerKey);

        // 1. YEET BEP-20 Token
        YeetToken token = new YeetToken(deployer, rewardPool);
        console.log("YeetToken:   ", address(token));

        // 2. Tipping (10% platform fee, fee wallet = deployer on testnet)
        YeetTipping tipping = new YeetTipping(address(token), deployer, deployer);
        console.log("YeetTipping: ", address(tipping));

        // 3. NFT posts
        YeetNFT nft = new YeetNFT(deployer);
        console.log("YeetNFT:     ", address(nft));

        vm.stopBroadcast();

        console.log("\n=== Deployment Complete ===");
        console.log("Explorer:");
        if (block.chainid == 97) {
            console.log("  https://testnet.bscscan.com/address/", address(token));
            console.log("  https://testnet.bscscan.com/address/", address(tipping));
            console.log("  https://testnet.bscscan.com/address/", address(nft));
        } else {
            console.log("  https://bscscan.com/address/", address(token));
            console.log("  https://bscscan.com/address/", address(tipping));
            console.log("  https://bscscan.com/address/", address(nft));
        }
    }
}