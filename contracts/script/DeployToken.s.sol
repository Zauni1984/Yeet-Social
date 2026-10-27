// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Script.sol";
import "../src/YeetToken.sol";

/// @notice Deploy ONLY the YEET token — the single contract the points model
///         needs (docs/mica/10). Payments / escrow / NFT contracts stay
///         undeployed until on-chain tips are actually switched on.
///
/// @dev Testnet:  forge script script/DeployToken.s.sol --rpc-url bsc_testnet --broadcast --verify -vvvv
/// @dev Mainnet:  forge script script/DeployToken.s.sol --rpc-url bsc_mainnet --broadcast --verify -vvvv
///
/// Env:
///   PRIVATE_KEY      deployer key (becomes owner; receives the 25 % initial tranches)
///   MINTER_ADDRESS   backend minter address (the address of REWARDS_MINTER_PRIVKEY);
///                    default: deployer — fine on testnet, never on mainnet
///   DAILY_MINT_CAP   daily mint ceiling in whole YEET (default 1_000_000; 0 = uncapped)
///   OWNER_ADDRESS    optional multisig to hand ownership to (two-step: the multisig
///                    must call acceptOwnership(); until then the deployer stays owner)
contract DeployToken is Script {
    function run() external {
        uint256 deployerKey = vm.envUint("PRIVATE_KEY");
        address deployer    = vm.addr(deployerKey);
        address minter      = vm.envOr("MINTER_ADDRESS", deployer);
        uint256 capYeet     = vm.envOr("DAILY_MINT_CAP", uint256(1_000_000));
        address newOwner    = vm.envOr("OWNER_ADDRESS", address(0));

        console.log("Deployer / owner: ", deployer);
        console.log("Minter:           ", minter);
        console.log("Daily mint cap:   ", capYeet, "YEET");
        console.log("Chain ID:         ", block.chainid);
        console.log("Network:          ", block.chainid == 97  ? "BNB Smart Chain Testnet"
                                        : block.chainid == 56  ? "BNB Smart Chain Mainnet"
                                        : "Unknown");
        if (block.chainid == 56) {
            require(minter != deployer, "mainnet: MINTER_ADDRESS must not be the deployer key");
            require(capYeet != 0, "mainnet: DAILY_MINT_CAP must be set");
        }

        vm.startBroadcast(deployerKey);
        YeetToken token = new YeetToken(deployer, minter, capYeet * 1e18);
        if (newOwner != address(0)) {
            token.transferOwnership(newOwner); // pending until the multisig accepts
            console.log("Ownership offered to:", newOwner, "(call acceptOwnership from there)");
        }
        vm.stopBroadcast();

        console.log("YeetToken:        ", address(token));
        console.log("\nBackend .env:");
        console.log("  YEET_TOKEN_ADDRESS=", address(token));
        console.log("  YEET_CHAIN_ID=", block.chainid);
        console.log("  REWARDS_MINTER_PRIVKEY=<private key of the minter above>");
    }
}
