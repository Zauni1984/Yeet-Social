// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "forge-std/Script.sol";
import "../src/YeetToken.sol";
import "../src/YeetTipping.sol";
import "../src/YeetNFT.sol";
import "../src/YeetPayments.sol";
import "../src/PaperWalletEscrow.sol";

/// @notice Deploy all Yeet contracts
/// @dev Testnet:  forge script script/Deploy.s.sol --rpc-url bsc_testnet --broadcast --verify -vvvv
/// @dev Mainnet:  forge script script/Deploy.s.sol --rpc-url bsc_mainnet --broadcast --verify -vvvv
///
/// Env:
///   PRIVATE_KEY        deployer key
///   REWARD_POOL        reward pool address (default: deployer)
///   PLATFORM_WALLET    fee recipient for payments (default: deployer)
///   VOUCHER_MAX        max YEET per paper-wallet voucher, in whole YEET
///                      (default 150). 0 = unlimited.
contract Deploy is Script {
    function run() external {
        uint256 deployerKey    = vm.envUint("PRIVATE_KEY");
        address deployer       = vm.addr(deployerKey);
        address rewardPool     = vm.envOr("REWARD_POOL", deployer);
        address platformWallet = vm.envOr("PLATFORM_WALLET", deployer);
        uint256 voucherMaxYeet = vm.envOr("VOUCHER_MAX", uint256(150));

        // Paper-wallet validity window: 1 hour .. 365 days.
        uint64 minValidity = 1 hours;
        uint64 maxValidity = 365 days;

        console.log("Deployer:    ", deployer);
        console.log("Chain ID:    ", block.chainid);
        console.log("Network:     ", block.chainid == 97  ? "BNB Smart Chain Testnet"
                                   : block.chainid == 56  ? "BNB Smart Chain Mainnet"
                                   : "Unknown");
        console.log("Reward pool: ", rewardPool);
        console.log("Platform:    ", platformWallet);

        vm.startBroadcast(deployerKey);

        // 1. YEET BEP-20 Token
        YeetToken token = new YeetToken(deployer, rewardPool);
        console.log("YeetToken:        ", address(token));

        // 2. Legacy tipping (kept for compatibility; superseded by YeetPayments).
        YeetTipping tipping = new YeetTipping(address(token), platformWallet, deployer);
        console.log("YeetTipping:      ", address(tipping));

        // 3. NFT posts
        YeetNFT nft = new YeetNFT(deployer);
        console.log("YeetNFT:          ", address(nft));

        // 4. Unified non-custodial payments (tips + PPV + promotion).
        YeetPayments payments = new YeetPayments(address(token), platformWallet, deployer);
        console.log("YeetPayments:     ", address(payments));

        // 5. Paper-wallet escrow (bearer vouchers).
        PaperWalletEscrow escrow = new PaperWalletEscrow(
            address(token),
            platformWallet,
            deployer,
            voucherMaxYeet == 0 ? 0 : voucherMaxYeet * 1e18,
            minValidity,
            maxValidity
        );
        console.log("PaperWalletEscrow:", address(escrow));

        vm.stopBroadcast();

        console.log("\n=== Deployment Complete ===");
        console.log("Set these in the backend .env and frontend window.YEET_CHAIN:");
        console.log("  YEET_TOKEN_ADDRESS      =", address(token));
        console.log("  YEET_NFT_ADDRESS        =", address(nft));
        console.log("  YEET_PAYMENTS_ADDRESS   =", address(payments));
        console.log("  YEET_PAPER_ESCROW_ADDRESS=", address(escrow));
        console.log("\nNEXT (before production):");
        console.log("  - transfer ownership of token/payments/escrow to a multisig (Ownable2Step)");
        console.log("  - run an external audit (docs/mica/07 checklist)");
    }
}
