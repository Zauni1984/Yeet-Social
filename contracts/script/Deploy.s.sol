// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Script.sol";
import "../YeetToken.sol";
import "../YeetNFT.sol";

/**
 * @notice Deploy YeetToken (BEP-20) and YeetNFT (BEP-721) to BSC.
 *
 * Usage:
 *   # BSC Testnet
 *   forge script contracts/script/Deploy.s.sol:DeployYeet \
 *     --rpc-url bsc_testnet \
 *     --broadcast \
 *     --verify \
 *     -vvvv
 *
 *   # BSC Mainnet
 *   forge script contracts/script/Deploy.s.sol:DeployYeet \
 *     --rpc-url bsc_mainnet \
 *     --broadcast \
 *     --verify \
 *     -vvvv
 *
 * Set in .env:
 *   PRIVATE_KEY=0x...
 *   TEAM_WALLET=0x...
 *   ECOSYSTEM_WALLET=0x...
 *   LIQUIDITY_WALLET=0x...
 *   REWARDS_MINTER=0x...   (backend hot wallet)
 */
contract DeployYeet is Script {

    function run() external {
        uint256 deployerKey = vm.envUint("PRIVATE_KEY");
        address deployer     = vm.addr(deployerKey);
        address team         = vm.envAddress("TEAM_WALLET");
        address ecosystem    = vm.envAddress("ECOSYSTEM_WALLET");
        address liquidity    = vm.envAddress("LIQUIDITY_WALLET");
        address minter       = vm.envAddress("REWARDS_MINTER");

        console.log("Deployer:   ", deployer);
        console.log("Team:       ", team);
        console.log("Ecosystem:  ", ecosystem);
        console.log("Liquidity:  ", liquidity);
        console.log("Minter:     ", minter);

        vm.startBroadcast(deployerKey);

        // 1. Deploy YEET Token (BEP-20)
        YeetToken token = new YeetToken(team, ecosystem, liquidity);
        console.log("YeetToken:  ", address(token));

        // 2. Authorize backend hot wallet as reward minter
        token.setRewardsMinter(minter);

        // 3. Deploy YeetNFT (BEP-721)
        YeetNFT nft = new YeetNFT(address(token));
        console.log("YeetNFT:    ", address(nft));

        vm.stopBroadcast();

        // Print .env values to copy
        console.log("\n--- Copy these into your .env ---");
        console.log("YEET_TOKEN_ADDRESS=", address(token));
        console.log("YEET_NFT_ADDRESS=",   address(nft));
    }
}
