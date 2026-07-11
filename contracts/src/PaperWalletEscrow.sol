// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/access/Ownable2Step.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import "@openzeppelin/contracts/utils/cryptography/MessageHashUtils.sol";

/// @title PaperWalletEscrow — bearer YEET vouchers, non-custodial
/// @notice An issuer locks their OWN YEET into a voucher keyed by an
///         ephemeral public-key address. Whoever holds the matching
///         ephemeral private key (embedded in the paper-wallet QR) can
///         redeem it to any recipient. After expiry the locked amount is
///         refundable to the issuer.
///
/// Why this satisfies the guardrails (docs/mica/06 L5 + docs/mica/07):
///  - **Not upgradeable**: plain contract, no proxy, no delegatecall.
///  - **No admin sweep**: the owner can NEVER move escrowed funds. There is
///    no withdraw/rescue function. Owner can only tune non-custodial config
///    (fee, limits) and pause NEW issuance.
///  - **claim/refund always available**: pausing only blocks `create`.
///  - **Front-running safe**: redemption reveals the secret in the mempool,
///    so a naive hash-lock could be stolen by a bot that copies the secret
///    and claims to its own address. Here the holder signs the *recipient*
///    with the ephemeral key; the signature binds the payout address, so a
///    front-runner cannot redirect the funds without the private key.
///
/// MiCA note: the funds rest in the contract, not with the platform, and the
/// platform holds no key that can move them → this is escrow-by-code, not
/// custody by the operator.
contract PaperWalletEscrow is Ownable2Step, ReentrancyGuard {
    using SafeERC20 for IERC20;
    using ECDSA for bytes32;
    using MessageHashUtils for bytes32;

    IERC20 public immutable yeetToken;

    /// @notice Optional platform fee on redemption, basis points. Default 0
    ///         (vouchers are gifts). Config only — cannot touch escrow.
    address public platformWallet;
    uint16 public platformFeeBps;
    uint16 public constant MAX_FEE_BPS = 1000; // hard cap 10%

    /// @notice AML/UX bounds on a single voucher (config only).
    uint256 public maxVoucherAmount;          // 0 = unlimited
    uint256 public constant MIN_AMOUNT = 1e18; // 1 YEET
    uint64 public minValidity;                 // min seconds until expiry
    uint64 public maxValidity;                 // max seconds until expiry

    /// @notice Blocks `create` only. claim/refund are never pausable.
    bool public issuancePaused;

    struct Voucher {
        address issuer; // 160 bits ┐ one slot
        uint96 amount;  //  96 bits ┘
        uint64 expiry;  //           ┐ next slot
        bool redeemed;  //           ┘ (claimed or refunded)
    }

    /// @dev Keyed by the ephemeral claim address (uniquely identifies a voucher).
    mapping(address => Voucher) public vouchers;

    event VoucherCreated(address indexed claimAddr, address indexed issuer, uint256 amount, uint64 expiry);
    event VoucherClaimed(address indexed claimAddr, address indexed recipient, uint256 net, uint256 fee);
    event VoucherRefunded(address indexed claimAddr, address indexed issuer, uint256 amount);
    event PlatformWalletUpdated(address indexed oldWallet, address indexed newWallet);
    event PlatformFeeUpdated(uint16 oldBps, uint16 newBps);
    event LimitsUpdated(uint256 maxVoucherAmount, uint64 minValidity, uint64 maxValidity);
    event IssuancePaused(bool paused);

    error ZeroAddress();
    error VoucherExists();
    error VoucherUnknown();
    error AlreadyRedeemed();
    error Expired();
    error NotYetExpired();
    error BelowMinimum();
    error AboveMaximum();
    error BadValidity();
    error BadSignature();
    error Paused();
    error FeeTooHigh();
    error AmountOverflow();

    constructor(
        address _yeetToken,
        address _platformWallet,
        address _owner,
        uint256 _maxVoucherAmount,
        uint64 _minValidity,
        uint64 _maxValidity
    ) Ownable(_owner) {
        if (_yeetToken == address(0) || _platformWallet == address(0)) revert ZeroAddress();
        if (_minValidity == 0 || _maxValidity < _minValidity) revert BadValidity();
        yeetToken = IERC20(_yeetToken);
        platformWallet = _platformWallet;
        platformFeeBps = 0; // vouchers are gifts by default
        maxVoucherAmount = _maxVoucherAmount;
        minValidity = _minValidity;
        maxValidity = _maxValidity;
    }

    // ── Issue ───────────────────────────────────────────────────────────

    /// @notice Lock `amount` YEET into a new voucher.
    /// @param claimAddr Ethereum address of the EPHEMERAL keypair whose
    ///                  private key is printed/encoded in the paper wallet.
    ///                  Must be fresh (no existing voucher).
    /// @param amount    YEET to lock (>= MIN_AMOUNT, <= maxVoucherAmount).
    /// @param expiry    Absolute unix time; refundable to issuer afterwards.
    /// Requires prior approve() of `amount` to this contract.
    function create(address claimAddr, uint256 amount, uint64 expiry)
        external
        nonReentrant
    {
        if (issuancePaused) revert Paused();
        if (claimAddr == address(0)) revert ZeroAddress();
        if (vouchers[claimAddr].issuer != address(0)) revert VoucherExists();
        if (amount < MIN_AMOUNT) revert BelowMinimum();
        if (maxVoucherAmount != 0 && amount > maxVoucherAmount) revert AboveMaximum();
        if (amount > type(uint96).max) revert AmountOverflow();

        uint256 delta = expiry <= block.timestamp ? 0 : expiry - block.timestamp;
        if (delta < minValidity || delta > maxValidity) revert BadValidity();

        vouchers[claimAddr] = Voucher({
            issuer: msg.sender,
            amount: uint96(amount),
            expiry: expiry,
            redeemed: false
        });

        // Pull the issuer's own tokens into escrow.
        yeetToken.safeTransferFrom(msg.sender, address(this), amount);

        emit VoucherCreated(claimAddr, msg.sender, amount, expiry);
    }

    // ── Redeem ──────────────────────────────────────────────────────────

    /// @notice Redeem a voucher to `recipient`. The `signature` must be made
    ///         by the ephemeral private key over `recipient` (domain-bound),
    ///         which is what prevents mempool front-running of the secret.
    /// @param claimAddr  The voucher key (ephemeral address).
    /// @param recipient  Where the net amount goes (a self-custody wallet).
    /// @param signature  ECDSA sig by the ephemeral key over the claim digest.
    function claim(address claimAddr, address recipient, bytes calldata signature)
        external
        nonReentrant
    {
        if (recipient == address(0)) revert ZeroAddress();
        Voucher storage v = vouchers[claimAddr];
        if (v.issuer == address(0)) revert VoucherUnknown();
        if (v.redeemed) revert AlreadyRedeemed();
        if (block.timestamp >= v.expiry) revert Expired();

        // Domain-bound digest: ties the signature to THIS voucher, THIS
        // recipient, THIS contract and chain — no cross-context replay.
        bytes32 digest = keccak256(
            abi.encode(
                keccak256("YeetPaperWalletClaim(address claimAddr,address recipient,uint256 chainId,address verifyingContract)"),
                claimAddr,
                recipient,
                block.chainid,
                address(this)
            )
        ).toEthSignedMessageHash();

        address signer = digest.recover(signature);
        if (signer != claimAddr) revert BadSignature();

        uint256 amount = v.amount;
        uint256 fee = (amount * platformFeeBps) / 10_000;
        uint256 net = amount - fee;

        // Effects before interactions.
        v.redeemed = true;

        if (fee > 0) yeetToken.safeTransfer(platformWallet, fee);
        yeetToken.safeTransfer(recipient, net);

        emit VoucherClaimed(claimAddr, recipient, net, fee);
    }

    /// @notice After expiry, return the locked amount to the issuer. Callable
    ///         by anyone (in case the issuer lost the ephemeral key), but the
    ///         funds always go to the recorded issuer — never elsewhere.
    function refund(address claimAddr) external nonReentrant {
        Voucher storage v = vouchers[claimAddr];
        if (v.issuer == address(0)) revert VoucherUnknown();
        if (v.redeemed) revert AlreadyRedeemed();
        if (block.timestamp < v.expiry) revert NotYetExpired();

        uint256 amount = v.amount;
        address issuer = v.issuer;

        v.redeemed = true;
        yeetToken.safeTransfer(issuer, amount);

        emit VoucherRefunded(claimAddr, issuer, amount);
    }

    /// @notice The digest a claimer must sign with the ephemeral key. Exposed
    ///         so clients build the exact bytes the contract verifies.
    function claimDigest(address claimAddr, address recipient) external view returns (bytes32) {
        return keccak256(
            abi.encode(
                keccak256("YeetPaperWalletClaim(address claimAddr,address recipient,uint256 chainId,address verifyingContract)"),
                claimAddr,
                recipient,
                block.chainid,
                address(this)
            )
        ).toEthSignedMessageHash();
    }

    // ── Admin (config + issuance pause ONLY — never moves escrow) ───────

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

    function setLimits(uint256 _maxVoucherAmount, uint64 _minValidity, uint64 _maxValidity)
        external
        onlyOwner
    {
        if (_minValidity == 0 || _maxValidity < _minValidity) revert BadValidity();
        maxVoucherAmount = _maxVoucherAmount;
        minValidity = _minValidity;
        maxValidity = _maxValidity;
        emit LimitsUpdated(_maxVoucherAmount, _minValidity, _maxValidity);
    }

    function setIssuancePaused(bool _paused) external onlyOwner {
        issuancePaused = _paused;
        emit IssuancePaused(_paused);
    }
}
