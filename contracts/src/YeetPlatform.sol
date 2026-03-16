// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "./YeetToken.sol";

/**
 * @title YeetPlatform
 * @notice Handles post rewards, fee collection, and tip payments.
 *
 * Fee schedule (in YEET):
 *  - Create post        : free  (earns +5 YEET reward)
 *  - NFT post           : 10 YEET fee  → 9 to treasury, 1 burned
 *  - Pay-per-view post  : set by creator, 10% platform cut
 *  - Tip                : any amount, 5% platform cut, 95% to creator
 */
contract YeetPlatform is Ownable, ReentrancyGuard {

    YeetToken public immutable yeet;

    uint256 public constant POST_REWARD      =   5 * 10**18;  //  5 YEET per post
    uint256 public constant LIKE_REWARD      =   1 * 10**18;  //  1 YEET per like received
    uint256 public constant NFT_POST_FEE     =  10 * 10**18;  // 10 YEET to mint NFT post
    uint256 public constant PLATFORM_FEE_BPS = 500;           //  5% (basis points)
    uint256 public constant BURN_BPS         = 100;           //  1% of NFT fee burned

    event PostRewarded(address indexed user, uint256 reward);
    event LikeRewarded(address indexed author, uint256 reward);
    event TipSent(address indexed from, address indexed to, uint256 amount, uint256 fee);
    event NFTPostFeeCollected(address indexed user, uint256 fee, uint256 burned);

    constructor(address _yeet) Ownable(msg.sender) {
        yeet = YeetToken(_yeet);
    }

    // ── Rewards ────────────────────────────────────────────────────────────
    /// @notice Called by backend (trusted) when a user creates a post.
    ///         The backend verifies the wallet signature before calling.
    function rewardPost(address user) external onlyOwner nonReentrant {
        require(user != address(0), "zero");
        uint256 bal = yeet.balanceOf(address(this));
        uint256 reward = bal >= POST_REWARD ? POST_REWARD : bal;
        if (reward > 0) {
            yeet.transfer(user, reward);
            emit PostRewarded(user, reward);
        }
    }

    /// @notice Called by backend when a post gets a like.
    function rewardLike(address author) external onlyOwner nonReentrant {
        require(author != address(0), "zero");
        uint256 bal = yeet.balanceOf(address(this));
        uint256 reward = bal >= LIKE_REWARD ? LIKE_REWARD : bal;
        if (reward > 0) {
            yeet.transfer(author, reward);
            emit LikeRewarded(author, reward);
        }
    }

    // ── Fee payment ────────────────────────────────────────────────────────
    /// @notice Pay the NFT post fee. Part goes to treasury, part burned.
    function payNFTPostFee() external nonReentrant {
        yeet.transferFrom(msg.sender, address(this), NFT_POST_FEE);
        uint256 burnAmt = (NFT_POST_FEE * BURN_BPS) / 10000;
        uint256 treasuryAmt = NFT_POST_FEE - burnAmt;
        yeet.burnToDead(burnAmt);
        yeet.transfer(yeet.treasury(), treasuryAmt);
        emit NFTPostFeeCollected(msg.sender, treasuryAmt, burnAmt);
    }

    // ── Tip ────────────────────────────────────────────────────────────────
    /// @notice Tip a post creator. 5% goes to treasury.
    function tip(address creator, uint256 amount) external nonReentrant {
        require(creator != address(0) && creator != msg.sender, "invalid creator");
        require(amount > 0, "zero amount");
        yeet.transferFrom(msg.sender, address(this), amount);
        uint256 fee = (amount * PLATFORM_FEE_BPS) / 10000;
        uint256 payout = amount - fee;
        yeet.transfer(creator, payout);
        yeet.transfer(yeet.treasury(), fee);
        emit TipSent(msg.sender, creator, payout, fee);
    }

    // ── Admin ──────────────────────────────────────────────────────────────
    /// @notice Rescue any tokens accidentally sent here (not YEET reward pool).
    function rescueTokens(address token, uint256 amount) external onlyOwner {
        IERC20(token).transfer(msg.sender, amount);
    }

    interface IERC20 {
        function transfer(address to, uint256 amount) external returns (bool);
    }
}