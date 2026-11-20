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
///   PRIVATE_KEY      deployer key (owner until the multisig accepts ownership)
///   MINTER_ADDRESS   backend minter address (the address of REWARDS_MINTER_PRIVKEY);
///                    default: deployer — fine on testnet, never on mainnet
///   DAILY_MINT_CAP   daily mint ceiling in whole YEET (default 1_000_000;
///                    1..78_750_000 = 0.5 % of the reward reserve)
///   OWNER_ADDRESS    multisig to hand ownership to (two-step: the multisig must call
///                    acceptOwnership(); until then the deployer stays owner).
///                    Required on mainnet and must be a contract.
///   TREASURY_ADDRESS receives the 25 % initial tranches. Default: OWNER_ADDRESS, else
///                    the deployer. On mainnet it must be a contract (multisig/vesting).
///   GUARDIAN_ADDRESS optional key that may pause minting (never resume)
contract DeployToken is Script {
    function run() external {
        uint256 deployerKey = vm.envUint("PRIVATE_KEY");
        address deployer    = vm.addr(deployerKey);
        address minter      = vm.envOr("MINTER_ADDRESS", deployer);
        uint256 capYeet     = vm.envOr("DAILY_MINT_CAP", uint256(1_000_000));
        address newOwner    = vm.envOr("OWNER_ADDRESS", address(0));
        address treasury    = vm.envOr("TREASURY_ADDRESS", newOwner == address(0) ? deployer : newOwner);
        address guardian    = vm.envOr("GUARDIAN_ADDRESS", address(0));

        console.log("Deployer / owner: ", deployer);
        console.log("Minter:           ", minter);
        console.log("Treasury (25 %):  ", treasury);
        console.log("Guardian:         ", guardian);
        console.log("Daily mint cap:   ", capYeet, "YEET");
        console.log("Chain ID:         ", block.chainid);
        console.log("Network:          ", block.chainid == 97  ? "BNB Smart Chain Testnet"
                                        : block.chainid == 56  ? "BNB Smart Chain Mainnet"
                                        : "Unknown");
        if (block.chainid == 56) {
            require(minter != deployer, "mainnet: MINTER_ADDRESS must not be the deployer key");
            // One stolen key must never be enough: owner and treasury are multisig
            // contracts, and the hot minter key is neither of them.
            require(newOwner != address(0), "mainnet: OWNER_ADDRESS (multisig) is required");
            require(newOwner.code.length > 0, "mainnet: OWNER_ADDRESS must be a contract (multisig)");
            require(treasury.code.length > 0, "mainnet: TREASURY_ADDRESS must be a contract (multisig)");
            require(minter != newOwner && minter != treasury, "mainnet: minter must be a separate key");
            require(guardian != minter, "mainnet: guardian must not be the minter key");
        }

        vm.startBroadcast(deployerKey);
        YeetToken token = new YeetToken(deployer, treasury, minter, capYeet * 1e18);
        if (guardian != address(0)) token.setGuardian(guardian);
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
