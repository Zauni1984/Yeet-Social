// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/access/Ownable2Step.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/utils/Pausable.sol";

/// @title YeetPayments — non-custodial wallet-to-wallet YEET payments
/// @notice Unifies tips, pay-per-view unlocks and post promotions. Every
///         payment moves YEET **directly** from the payer's wallet to the
///         recipient (and the platform fee wallet) in a single atomic
///         transfer. The contract never holds user funds — see
///         docs/mica/06 (L1/L2) and docs/mica/07.
///
/// MiCA note: because the payer signs and the tokens never rest with the
/// platform, this is not custody, not a transfer service "for clients" and
/// not an exchange service. The platform is a software provider that
/// collects a fee. Recipients MUST be self-custody wallets — if a creator
/// has no wallet the caller pays in off-chain points instead (enforced
/// off-chain), never by crediting incoming YEET internally.
contract YeetPayments is Ownable2Step, ReentrancyGuard, Pausable {
    using SafeERC20 for IERC20;

    IERC20 public immutable yeetToken;

    /// @notice Wallet that receives the platform fee.
    address public platformWallet;
    /// @notice Platform fee in basis points (1000 = 10%).
    uint16 public platformFeeBps;

    uint16 public constant MAX_FEE_BPS = 2000; // hard cap 20%
    uint256 public constant MIN_AMOUNT = 1e18; // 1 YEET

    /// @dev Payment kinds, surfaced in events so the off-chain indexer can
    ///      route them (tip → counter, ppv → unlock, promo → boost).
    enum Kind {
        Tip,
        PayPerView,
        Promotion
    }

    // Aggregate stats (informational; the indexer is source of truth).
    mapping(bytes32 => uint256) public refTotal; // ref (postId/contentId) => gross paid
    mapping(address => uint256) public totalSent;
    mapping(address => uint256) public totalReceived;

    event Paid(
        Kind indexed kind,
        address indexed payer,
        address indexed recipient,
        bytes32 ref, // off-chain UUID as bytes32 (post/content/promotion id)
        uint256 gross,
        uint256 fee,
        uint256 net
    );
    event PlatformWalletUpdated(address indexed oldWallet, address indexed newWallet);
    event PlatformFeeUpdated(uint16 oldBps, uint16 newBps);

    error ZeroAddress();
    error SelfPayment();
    error BelowMinimum();
    error FeeTooHigh();

    constructor(address _yeetToken, address _platformWallet, address _owner)
        Ownable(_owner)
    {
        if (_yeetToken == address(0) || _platformWallet == address(0)) revert ZeroAddress();
        yeetToken = IERC20(_yeetToken);
        platformWallet = _platformWallet;
        platformFeeBps = 1000; // 10%
    }

    // ── Payments ────────────────────────────────────────────────────────

    /// @notice Pay a recipient directly from the caller's wallet.
    /// @param kind      Tip / PayPerView / Promotion (routing hint for the indexer).
    /// @param recipient Creator wallet (self-custody). For Promotion this is
    ///                  typically the platform wallet — see docs/mica/07.
    /// @param ref       Off-chain reference (post/content/promotion UUID → bytes32).
    /// @param amount    Gross YEET amount (>= MIN_AMOUNT). Requires prior approve().
    ///
    /// The full flow is atomic: fee → platformWallet, net → recipient. No
    /// intermediate custody. Pausing only stops NEW payments; it cannot trap
    /// funds because nothing is ever held here.
    function pay(Kind kind, address recipient, bytes32 ref, uint256 amount)
        external
        nonReentrant
        whenNotPaused
    {
        _pay(kind, msg.sender, recipient, ref, amount);
    }

    /// @notice Convenience wrapper for a tip.
    function tip(address recipient, bytes32 postId, uint256 amount)
        external
        nonReentrant
        whenNotPaused
    {
        _pay(Kind.Tip, msg.sender, recipient, postId, amount);
    }

    /// @dev Single payment code path. `payer` is always the real caller —
    ///      never route this through an external `this.` call, which would
    ///      rewrite msg.sender to the contract and pull from the wrong wallet.
    function _pay(Kind kind, address payer, address recipient, bytes32 ref, uint256 amount)
        internal
    {
        if (recipient == address(0)) revert ZeroAddress();
        if (recipient == payer) revert SelfPayment();
        if (amount < MIN_AMOUNT) revert BelowMinimum();

        uint256 fee = (amount * platformFeeBps) / 10_000;
        uint256 net = amount - fee;

        // Effects before interactions.
        refTotal[ref] += amount;
        totalSent[payer] += amount;
        totalReceived[recipient] += net;

        // Two pulls from the payer; if either reverts the whole tx reverts,
        // so the split is always consistent.
        if (fee > 0) yeetToken.safeTransferFrom(payer, platformWallet, fee);
        yeetToken.safeTransferFrom(payer, recipient, net);

        emit Paid(kind, payer, recipient, ref, amount, fee, net);
    }

    /// @notice Quote the fee/net split for a gross amount (view helper for UIs).
    function quote(uint256 amount) external view returns (uint256 fee, uint256 net) {
        fee = (amount * platformFeeBps) / 10_000;
        net = amount - fee;
    }

    // ── Admin (config only — NEVER touches user funds) ──────────────────

    function setPlatformWallet(address _wallet) external onlyOwner {
        if (_wallet == address(0)) revert ZeroAddress();
        emit PlatformWalletUpdated(platformWallet, _wallet);
        platformWallet = _wallet;
    }

    function setPlatformFee(uint16 _bps) external onlyOwner {
        if (_bps > MAX_FEE_BPS) revert FeeTooHigh();
        emit PlatformFeeUpdated(platformFeeBps, _bps);
        platformFeeBps = _bps;
    }

    function pause() external onlyOwner { _pause(); }
    function unpause() external onlyOwner { _unpause(); }
}
