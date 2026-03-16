// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import "@openzeppelin/contracts/utils/cryptography/MessageHashUtils.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "./YeetToken.sol";

/**
 * @title YeetSwapVault
 * @notice Handles the one-time migration swap from the legacy chain token
 *         to the new BSC YEET token.
 *
 * Flow:
 *  1. User holds legacy YEET on old chain.
 *  2. User calls legacyBurn() on the old chain → emits BurnForSwap(user, amount).
 *  3. Backend (oracle) verifies the old-chain burn transaction.
 *  4. Backend calls claim() here with a signed proof.
 *  5. User receives equivalent YEET on BSC.
 *
 * Ratio: 1:1 (1 old YEET = 1 new YEET), but owner can set a custom ratio.
 * Unclaimed tokens after deadline can be burned to dead address.
 */
contract YeetSwapVault is Ownable, ReentrancyGuard {
    using ECDSA for bytes32;
    using MessageHashUtils for bytes32;

    YeetToken public immutable yeet;
    address   public oracle;       // Backend wallet that signs swap proofs
    uint256   public swapRatio;    // new per old, in basis points (10000 = 1:1)
    uint256   public swapDeadline; // Unix timestamp after which no more claims
    bool      public swapOpen;

    mapping(bytes32 => bool) public claimed; // oldChainTxHash => claimed

    event SwapClaimed(address indexed user, bytes32 indexed oldTxHash, uint256 oldAmount, uint256 newAmount);
    event SwapDeadlinePassed(uint256 unclaimed);
    event OracleSet(address indexed oracle);

    constructor(address _yeet, address _oracle) Ownable(msg.sender) {
        yeet = YeetToken(_yeet);
        oracle = _oracle;
        swapRatio = 10000; // 1:1 default
        swapDeadline = block.timestamp + 180 days; // 6 months to claim
        swapOpen = true;
    }

    // ── Claim ──────────────────────────────────────────────────────────────
    /**
     * @notice Claim new YEET tokens against a verified legacy burn.
     * @param user        Recipient of new tokens (must match signer expectation)
     * @param oldAmount   Amount burned on legacy chain (raw, 18 decimals)
     * @param oldTxHash   Transaction hash of the legacy burn (unique key)
     * @param signature   Oracle signature over (user, oldAmount, oldTxHash)
     */
    function claim(
        address user,
        uint256 oldAmount,
        bytes32 oldTxHash,
        bytes calldata signature
    ) external nonReentrant {
        require(swapOpen, "swap closed");
        require(block.timestamp <= swapDeadline, "deadline passed");
        require(!claimed[oldTxHash], "already claimed");
        require(user != address(0), "zero user");
        require(oldAmount > 0, "zero amount");

        // Verify oracle signature
        bytes32 hash = keccak256(abi.encodePacked(user, oldAmount, oldTxHash));
        bytes32 ethHash = hash.toEthSignedMessageHash();
        address signer = ethHash.recover(signature);
        require(signer == oracle, "invalid signature");

        claimed[oldTxHash] = true;

        uint256 newAmount = (oldAmount * swapRatio) / 10000;
        require(yeet.balanceOf(address(this)) >= newAmount, "vault empty");

        yeet.transfer(user, newAmount);
        emit SwapClaimed(user, oldTxHash, oldAmount, newAmount);
    }

    // ── Admin ──────────────────────────────────────────────────────────────
    function setOracle(address _oracle) external onlyOwner {
        require(_oracle != address(0), "zero");
        oracle = _oracle;
        emit OracleSet(_oracle);
    }

    function setSwapRatio(uint256 _bps) external onlyOwner {
        require(_bps > 0 && _bps <= 20000, "ratio out of range");
        swapRatio = _bps;
    }

    function closeSwap() external onlyOwner {
        swapOpen = false;
    }

    /// @notice After deadline, burn unclaimed tokens to dead address.
    function burnUnclaimed() external onlyOwner {
        require(block.timestamp > swapDeadline || !swapOpen, "swap still open");
        uint256 remaining = yeet.balanceOf(address(this));
        if (remaining > 0) {
            yeet.burnToDead(remaining);
            emit SwapDeadlinePassed(remaining);
        }
    }
}