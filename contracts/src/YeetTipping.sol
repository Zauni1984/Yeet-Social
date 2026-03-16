// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/utils/Pausable.sol";

/// @title YeetTipping — Wallet-to-wallet YEET tips with platform fee
/// @notice Users tip each other YEET tokens; platform takes a configurable cut
contract YeetTipping is Ownable, ReentrancyGuard, Pausable {
    using SafeERC20 for IERC20;

    IERC20 public immutable yeetToken;

    address public platformWallet;
    uint256 public platformFeeBps; // basis points: 1000 = 10%
    uint256 public constant MAX_FEE_BPS = 2000; // max 20%
    uint256 public constant MIN_TIP = 1 * 10 ** 18; // 1 YEET minimum

    // postId => total tips received
    mapping(bytes32 => uint256) public postTips;
    // user => total tips sent
    mapping(address => uint256) public totalSent;
    // user => total tips received
    mapping(address => uint256) public totalReceived;

    event Tipped(
        address indexed tipper,
        address indexed recipient,
        bytes32 indexed postId,
        uint256 tipAmount,
        uint256 fee,
        uint256 net
    );
    event PlatformWalletUpdated(address indexed oldWallet, address indexed newWallet);
    event PlatformFeeUpdated(uint256 oldFee, uint256 newFee);

    constructor(
        address _yeetToken,
        address _platformWallet,
        address _owner
    ) Ownable(_owner) {
        require(_yeetToken != address(0), "Invalid token");
        require(_platformWallet != address(0), "Invalid platform wallet");
        yeetToken = IERC20(_yeetToken);
        platformWallet = _platformWallet;
        platformFeeBps = 1000; // 10% default
    }

    /// @notice Tip a post creator
    /// @param recipient Address of the post author
    /// @param postId Off-chain post UUID (bytes32 hash)
    /// @param amount Amount of YEET to tip (must be >= MIN_TIP)
    function tip(
        address recipient,
        bytes32 postId,
        uint256 amount
    ) external nonReentrant whenNotPaused {
        require(recipient != address(0), "Invalid recipient");
        require(recipient != msg.sender, "Cannot tip yourself");
        require(amount >= MIN_TIP, "Below minimum tip");

        uint256 fee = (amount * platformFeeBps) / 10000;
        uint256 net = amount - fee;

        // Transfer from tipper (requires approval)
        yeetToken.safeTransferFrom(msg.sender, platformWallet, fee);
        yeetToken.safeTransferFrom(msg.sender, recipient, net);

        postTips[postId]     += amount;
        totalSent[msg.sender] += amount;
        totalReceived[recipient] += net;

        emit Tipped(msg.sender, recipient, postId, amount, fee, net);
    }

    // ── Admin ──────────────────────────────────────────────────

    function setPlatformWallet(address _wallet) external onlyOwner {
        require(_wallet != address(0), "Invalid address");
        emit PlatformWalletUpdated(platformWallet, _wallet);
        platformWallet = _wallet;
    }

    function setPlatformFee(uint256 _bps) external onlyOwner {
        require(_bps <= MAX_FEE_BPS, "Fee too high");
        emit PlatformFeeUpdated(platformFeeBps, _bps);
        platformFeeBps = _bps;
    }

    function pause() external onlyOwner { _pause(); }
    function unpause() external onlyOwner { _unpause(); }
}