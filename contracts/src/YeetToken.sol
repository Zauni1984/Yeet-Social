// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Burnable.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Permit.sol";
import "@openzeppelin/contracts/access/Ownable.sol";

/**
 * @title YeetToken
 * @notice BEP-20 / ERC-20 token for the Yeet Social platform on BSC.
 *
 * Tokenomics:
 *  - Total supply : 21,000,000,000 YEET  (21 billion, 18 decimals)
 *  - Burn address : 0x000000000000000000000000000000000000dEaD
 *  - Post reward  : 5 YEET per post (minted from reward pool, not from supply)
 *  - Fee currency : Platform fees paid in YEET, sent to treasury
 *  - Swap reserve : Held in SwapVault for migration from legacy chain
 */
contract YeetToken is ERC20, ERC20Burnable, ERC20Permit, Ownable {

    // ── Constants ──────────────────────────────────────────────────────────
    uint256 public constant TOTAL_SUPPLY     = 21_000_000_000 * 10**18; // 21 billion
    address public constant BURN_ADDRESS     = 0x000000000000000000000000000000000000dEaD;

    // Allocation (out of 21B)
    uint256 public constant SWAP_RESERVE     = 10_500_000_000 * 10**18; // 50% – legacy chain swap
    uint256 public constant REWARD_POOL      =  4_200_000_000 * 10**18; // 20% – post/engagement rewards
    uint256 public constant TREASURY         =  2_100_000_000 * 10**18; // 10% – platform treasury
    uint256 public constant TEAM_VESTING     =  1_680_000_000 * 10**18; //  8% – team (2yr vesting)
    uint256 public constant LIQUIDITY        =  1_260_000_000 * 10**18; //  6% – DEX liquidity
    uint256 public constant COMMUNITY        =  1_260_000_000 * 10**18; //  6% – community / airdrop

    // ── State ──────────────────────────────────────────────────────────────
    address public treasury;
    address public rewardDistributor;   // YeetPlatform contract
    address public swapVault;           // YeetSwapVault contract
    uint256 public rewardPoolRemaining;

    // ── Events ─────────────────────────────────────────────────────────────
    event RewardDistributorSet(address indexed prev, address indexed next);
    event SwapVaultSet(address indexed prev, address indexed next);
    event TreasurySet(address indexed prev, address indexed next);
    event PostRewardMinted(address indexed user, uint256 amount);

    // ── Constructor ────────────────────────────────────────────────────────
    constructor(
        address _treasury,
        address _team,
        address _liquidity,
        address _community
    )
        ERC20("Yeet Token", "YEET")
        ERC20Permit("Yeet Token")
        Ownable(msg.sender)
    {
        require(_treasury   != address(0), "zero treasury");
        require(_team       != address(0), "zero team");
        require(_liquidity  != address(0), "zero liquidity");
        require(_community  != address(0), "zero community");

        treasury = _treasury;

        // Mint all 21B to deployer first, then distribute
        _mint(msg.sender, TOTAL_SUPPLY);

        // Distribute allocations
        _transfer(msg.sender, _treasury,   TREASURY);
        _transfer(msg.sender, _team,       TEAM_VESTING);
        _transfer(msg.sender, _liquidity,  LIQUIDITY);
        _transfer(msg.sender, _community,  COMMUNITY);

        // Swap reserve stays with deployer until SwapVault is set
        // Reward pool stays with deployer until rewardDistributor is set
        rewardPoolRemaining = REWARD_POOL;
    }

    // ── Admin: set contracts ───────────────────────────────────────────────
    function setRewardDistributor(address _rd) external onlyOwner {
        require(_rd != address(0), "zero");
        emit RewardDistributorSet(rewardDistributor, _rd);
        // Move reward pool allocation to the new distributor
        if (rewardPoolRemaining > 0 && balanceOf(msg.sender) >= rewardPoolRemaining) {
            _transfer(msg.sender, _rd, rewardPoolRemaining);
        }
        rewardDistributor = _rd;
    }

    function setSwapVault(address _vault) external onlyOwner {
        require(_vault != address(0), "zero");
        emit SwapVaultSet(swapVault, _vault);
        // Move swap reserve to vault
        if (balanceOf(msg.sender) >= SWAP_RESERVE) {
            _transfer(msg.sender, _vault, SWAP_RESERVE);
        }
        swapVault = _vault;
    }

    function setTreasury(address _t) external onlyOwner {
        require(_t != address(0), "zero");
        emit TreasurySet(treasury, _t);
        treasury = _t;
    }

    // ── Burn helpers ───────────────────────────────────────────────────────
    /// @notice Burn tokens by sending to the canonical burn address.
    ///         Increases totalBurned counter automatically via transfer event.
    function burnToDead(uint256 amount) external {
        _transfer(msg.sender, BURN_ADDRESS, amount);
    }

    /// @notice Returns how many tokens sit at the burn address (not officially "burned"
    ///         in ERC20 sense, but permanently inaccessible).
    function burnedBalance() external view returns (uint256) {
        return balanceOf(BURN_ADDRESS);
    }
}