// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Burnable.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/Pausable.sol";

/**
 * @title YeetToken
 * @notice BEP-20 Utility Token for the Yeet Social Media Platform (BSC)
 *
 * Token Economics:
 * - Total supply: 1,000,000,000 YEET (1 billion)
 * - Platform rewards pool: 40% (400M) — minted gradually for user actions
 * - Team/Founders: 15% (150M) — 2-year vesting
 * - Ecosystem/Partnerships: 20% (200M)
 * - Liquidity: 15% (150M)
 * - Reserve: 10% (100M)
 *
 * Reward actions (off-chain tracked, batched on-chain):
 * - Daily login:     1.0 YEET
 * - Comment:         0.5 YEET
 * - Share:           0.5 YEET
 * - Reshare:         0.25 YEET
 * - Downvote:        0.1 YEET
 * - Mint NFT:        5.0 YEET
 * - Referral signup: 10.0 YEET
 */
contract YeetToken is ERC20, ERC20Burnable, Ownable, Pausable {

    // ─── Constants ────────────────────────────────────────────────────────────

    uint256 public constant MAX_SUPPLY = 1_000_000_000 * 10**18;
    uint256 public constant REWARDS_POOL = 400_000_000 * 10**18;

    // Platform revenue split
    uint8 public constant CRYPTO_FEE_PERCENT = 10;  // 10% for crypto tips
    uint8 public constant FIAT_FEE_PERCENT   = 20;  // 20% for fiat tips

    // ─── State ────────────────────────────────────────────────────────────────

    address public rewardsMinter;          // Backend hot wallet for batch rewards
    uint256 public totalRewardsMinted;

    // Vesting: team tokens locked until this timestamp
    uint256 public teamVestingUnlock;
    address public teamWallet;

    // Anti-spam: max rewards per wallet per day
    mapping(address => uint256) public dailyRewardsClaimed;
    mapping(address => uint256) public lastRewardDay;
    uint256 public constant MAX_DAILY_REWARDS = 50 * 10**18; // 50 YEET/day max

    // ─── Events ───────────────────────────────────────────────────────────────

    event RewardsMinted(address indexed to, uint256 amount, string action);
    event TipSent(address indexed from, address indexed to, uint256 amount, uint256 fee);
    event RewardsMinterUpdated(address indexed newMinter);

    // ─── Constructor ──────────────────────────────────────────────────────────

    constructor(
        address _teamWallet,
        address _ecosystemWallet,
        address _liquidityWallet
    ) ERC20("Yeet Token", "YEET") Ownable(msg.sender) {
        teamWallet = _teamWallet;
        teamVestingUnlock = block.timestamp + 730 days; // 2-year vesting

        // Mint initial allocations (team tokens stay locked in contract)
        _mint(address(this),       150_000_000 * 10**18); // Team (vested)
        _mint(_ecosystemWallet,    200_000_000 * 10**18); // Ecosystem
        _mint(_liquidityWallet,    150_000_000 * 10**18); // Liquidity
        _mint(owner(),             100_000_000 * 10**18); // Reserve
        // Rewards pool (400M) minted on-demand via mintRewards()
    }

    // ─── Modifiers ────────────────────────────────────────────────────────────

    modifier onlyMinter() {
        require(
            msg.sender == rewardsMinter || msg.sender == owner(),
            "YeetToken: not authorized minter"
        );
        _;
    }

    // ─── Reward Minting (called by backend batch job) ─────────────────────────

    /**
     * @notice Batch mint rewards for multiple users.
     *         Called by the backend hot wallet once per day/hour.
     * @param recipients  Array of wallet addresses
     * @param amounts     Corresponding YEET amounts (in wei)
     * @param actions     Human-readable action labels for events
     */
    function batchMintRewards(
        address[] calldata recipients,
        uint256[] calldata amounts,
        string[] calldata actions
    ) external onlyMinter whenNotPaused {
        require(
            recipients.length == amounts.length &&
            recipients.length == actions.length,
            "YeetToken: array length mismatch"
        );

        uint256 total = 0;
        for (uint i = 0; i < amounts.length; i++) {
            total += amounts[i];
        }
        require(
            totalRewardsMinted + total <= REWARDS_POOL,
            "YeetToken: rewards pool exhausted"
        );

        for (uint i = 0; i < recipients.length; i++) {
            _enforceAntiSpam(recipients[i], amounts[i]);
            _mint(recipients[i], amounts[i]);
            totalRewardsMinted += amounts[i];
            emit RewardsMinted(recipients[i], amounts[i], actions[i]);
        }
    }

    /**
     * @dev Enforce daily cap per wallet to prevent farming.
     */
    function _enforceAntiSpam(address user, uint256 amount) internal {
        uint256 today = block.timestamp / 1 days;
        if (lastRewardDay[user] < today) {
            dailyRewardsClaimed[user] = 0;
            lastRewardDay[user] = today;
        }
        require(
            dailyRewardsClaimed[user] + amount <= MAX_DAILY_REWARDS,
            "YeetToken: daily reward limit reached"
        );
        dailyRewardsClaimed[user] += amount;
    }

    // ─── On-chain Tipping ─────────────────────────────────────────────────────

    /**
     * @notice Send a YEET tip to a content creator.
     *         Platform fee is automatically deducted (10%).
     * @param creator   Recipient address
     * @param amount    Total tip amount in YEET wei
     */
    function tipCreator(
        address creator,
        uint256 amount
    ) external whenNotPaused {
        require(amount > 0, "YeetToken: amount must be > 0");
        require(creator != msg.sender, "YeetToken: cannot tip yourself");

        uint256 fee = (amount * CRYPTO_FEE_PERCENT) / 100;
        uint256 creatorAmount = amount - fee;

        // Transfer full amount from tipper
        _transfer(msg.sender, creator, creatorAmount);
        _transfer(msg.sender, owner(), fee); // Platform fee to treasury

        emit TipSent(msg.sender, creator, creatorAmount, fee);
    }

    // ─── Team Vesting ─────────────────────────────────────────────────────────

    function releaseTeamTokens() external {
        require(msg.sender == teamWallet, "YeetToken: not team wallet");
        require(
            block.timestamp >= teamVestingUnlock,
            "YeetToken: vesting period not over"
        );
        uint256 balance = balanceOf(address(this));
        require(balance > 0, "YeetToken: nothing to release");
        _transfer(address(this), teamWallet, balance);
    }

    // ─── Admin ────────────────────────────────────────────────────────────────

    function setRewardsMinter(address _minter) external onlyOwner {
        rewardsMinter = _minter;
        emit RewardsMinterUpdated(_minter);
    }

    function pause()   external onlyOwner { _pause(); }
    function unpause() external onlyOwner { _unpause(); }

    function _update(address from, address to, uint256 value)
        internal override whenNotPaused
    {
        super._update(from, to, value);
    }
}
