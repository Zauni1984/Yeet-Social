// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Burnable.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Permit.sol";
import "@openzeppelin/contracts/access/Ownable2Step.sol";

/// @title YeetToken — BEP-20 utility token for Yeet Social on BSC
/// @notice Fixed maximum supply of 21,000,000,000 YEET (docs/mica/02, Teil J).
///
///   Tranche                       Share   Amount            Issued
///   ----------------------------- ------- ----------------- -------------------------------
///   Developer                      10 %    2,100,000,000     at deploy, to `initialOwner`
///   Team                           10 %    2,100,000,000     at deploy, to `initialOwner`
///   Reserve (liquidity, listings)   5 %    1,050,000,000     at deploy, to `initialOwner`
///   Rewards / community pool       75 %   15,750,000,000     gradually, ONLY via
///                                                            `batchMintRewards` (points→YEET
///                                                            conversions, see backend
///                                                            `services/batch_rewards.rs`)
///
/// This is deliberately the ONLY contract the platform needs while points live
/// off-chain (docs/mica/10): the platform economy (rewards, tips, pay-per-view,
/// paper wallets) runs entirely in the backend ledger; the chain is touched once
/// per approved conversion, when the backend mints the YEET to the user's own
/// wallet. Nothing is ever held by the platform on-chain.
///
/// Roles
///   * `owner`  — the multisig (Ownable2Step hand-over). Sets the minter, the
///                guardian, the daily cap and the mint pause. Never mints itself
///                and cannot be renounced (a contract without owner could never
///                rotate a leaked minter key again).
///   * `guardian` — optional fast key that can do exactly one thing: pause
///                minting. Resuming needs the owner. Lets one person stop a
///                leaked minter within minutes while the multisig collects
///                signatures.
///   * `minter` — the backend hot key. Can do exactly one thing: mint reward
///                tranche YEET to recipients, bounded by `REWARD_RESERVE`,
///                by `MAX_BATCH` per call and by `dailyMintCap` per UTC day.
///                The cap is mandatory and can never exceed `MAX_DAILY_MINT_CAP`
///                (0.5 % of the reserve), so a leaked minter key drains at most
///                one day's cap before minting is paused or the key rotated —
///                and even a compromised owner cannot empty the reserve at once.
///
/// Moving to another chain or a successor token later needs no upgrade hook
/// here: holders approve (or `permit`) a successor contract, which calls
/// `burnFrom` on this token and issues the new one 1:1 (contracts/README.md).
///
/// The reward tranche is the only thing that can ever be minted after deployment, it is
/// bounded by `REWARD_RESERVE` independently of burns, and there is no other mint function —
/// so `totalSupply()` can never exceed `MAX_SUPPLY`. The backend's conversion pool
/// (`YEET_CONVERSION_POOL`, default = `REWARD_RESERVE`) mirrors this number off-chain.
contract YeetToken is ERC20, ERC20Burnable, ERC20Permit, Ownable2Step {
    uint256 public constant MAX_SUPPLY      = 21_000_000_000 * 10 ** 18;
    uint256 public constant DEVELOPER_SHARE = (MAX_SUPPLY * 10) / 100;
    uint256 public constant TEAM_SHARE      = (MAX_SUPPLY * 10) / 100;
    uint256 public constant RESERVE_SHARE   = (MAX_SUPPLY * 5) / 100;
    /// @notice The 75 % rewards/community tranche: the only mintable amount after deploy.
    uint256 public constant REWARD_RESERVE  = MAX_SUPPLY - DEVELOPER_SHARE - TEAM_SHARE - RESERVE_SHARE;
    /// @notice Absolute ceiling for `dailyMintCap`: 0.5 % of the reward reserve per UTC day,
    ///         i.e. the reserve can never be minted out in fewer than 200 days.
    uint256 public constant MAX_DAILY_MINT_CAP = REWARD_RESERVE / 200;
    /// @notice Maximum recipients per `batchMintRewards` call (gas bound).
    uint256 public constant MAX_BATCH = 200;

    /// @notice Cumulative YEET minted from the reward reserve (monotonic; burns do not lower it).
    uint256 public rewardsMinted;

    /// @notice The only address allowed to mint (backend batch minter). Zero = minting disabled.
    address public minter;
    /// @notice Address that may pause minting besides the owner. Zero = none.
    address public guardian;
    /// @notice Ceiling per UTC day for the minter, in wei. Always 1..MAX_DAILY_MINT_CAP.
    uint256 public dailyMintCap;
    /// @notice Emergency stop for minting (owner). Transfers are never paused.
    bool public mintingPaused;
    /// @dev day index (block.timestamp / 1 day) → wei minted that day.
    mapping(uint256 => uint256) public mintedOnDay;

    event RewardMinted(address indexed recipient, uint256 amount, string action);
    event MinterUpdated(address indexed oldMinter, address indexed newMinter);
    event GuardianUpdated(address indexed oldGuardian, address indexed newGuardian);
    event DailyMintCapUpdated(uint256 oldCap, uint256 newCap);
    event MintingPaused(bool paused);

    error NotMinter();
    error MintingIsPaused();
    error DailyCapExceeded(uint256 requested, uint256 remainingToday);
    error InvalidDailyCap(uint256 requested, uint256 maximum);
    error NotOwnerOrGuardian();
    error RenounceDisabled();
    error ZeroTreasury();

    modifier onlyMinter() {
        if (msg.sender != minter) revert NotMinter();
        _;
    }

    /// @param initialOwner  owns the contract (hand over to the multisig with Ownable2Step)
    /// @param treasury      receives the 25 % initial tranches (the multisig in production;
    ///                      kept separate from the owner so the deployer key never holds them)
    /// @param initialMinter backend minter key (may be zero and set later via `setMinter`)
    /// @param initialDailyCap daily mint ceiling in wei (1..MAX_DAILY_MINT_CAP)
    constructor(address initialOwner, address treasury, address initialMinter, uint256 initialDailyCap)
        ERC20("Yeet Token", "YEET")
        ERC20Permit("Yeet Token")
        Ownable(initialOwner)
    {
        // Developer + Team + Reserve (25 %). Vesting for Developer/Team is handled by
        // separate vesting contracts/multisig policy (docs/mica/02), not by this token.
        if (treasury == address(0)) revert ZeroTreasury();
        _checkDailyCap(initialDailyCap);
        _mint(treasury, DEVELOPER_SHARE + TEAM_SHARE + RESERVE_SHARE);
        minter = initialMinter;
        dailyMintCap = initialDailyCap;
        emit MinterUpdated(address(0), initialMinter);
        emit DailyMintCapUpdated(0, initialDailyCap);
    }

    /// @notice Decimals override (18, same as ETH)
    function decimals() public pure override returns (uint8) {
        return 18;
    }

    /// @notice YEET still mintable from the reward reserve.
    function rewardsRemaining() external view returns (uint256) {
        return REWARD_RESERVE - rewardsMinted;
    }

    /// @notice Current UTC day index used for the daily cap.
    function currentDay() public view returns (uint256) {
        return block.timestamp / 1 days;
    }

    /// @notice YEET the minter may still mint today.
    function dailyRemaining() public view returns (uint256) {
        uint256 used = mintedOnDay[currentDay()];
        return used >= dailyMintCap ? 0 : dailyMintCap - used;
    }

    // ── owner controls ──────────────────────────────────────────────────────

    /// @notice Rotate the backend minter key (zero disables minting).
    function setMinter(address newMinter) external onlyOwner {
        emit MinterUpdated(minter, newMinter);
        minter = newMinter;
    }

    /// @notice Set the address that may pause minting besides the owner (zero = none).
    function setGuardian(address newGuardian) external onlyOwner {
        emit GuardianUpdated(guardian, newGuardian);
        guardian = newGuardian;
    }

    /// @notice Set the per-day mint ceiling (wei), within 1..MAX_DAILY_MINT_CAP.
    function setDailyMintCap(uint256 newCap) external onlyOwner {
        _checkDailyCap(newCap);
        emit DailyMintCapUpdated(dailyMintCap, newCap);
        dailyMintCap = newCap;
    }

    /// @notice Emergency stop / resume for minting. Does not touch transfers.
    /// @dev Pausing: owner or guardian. Resuming: owner only.
    function setMintingPaused(bool paused) external {
        if (msg.sender != owner() && !(paused && msg.sender == guardian)) {
            if (paused) revert NotOwnerOrGuardian();
            revert OwnableUnauthorizedAccount(msg.sender);
        }
        mintingPaused = paused;
        emit MintingPaused(paused);
    }

    /// @notice Disabled: without an owner a leaked minter key could never be rotated.
    function renounceOwnership() public view override onlyOwner {
        revert RenounceDisabled();
    }

    function _checkDailyCap(uint256 cap) private pure {
        if (cap == 0 || cap > MAX_DAILY_MINT_CAP) revert InvalidDailyCap(cap, MAX_DAILY_MINT_CAP);
    }

    // ── minting ─────────────────────────────────────────────────────────────

    /// @notice Mint reward-tranche YEET to multiple addresses (points→YEET conversions).
    /// @dev Only the minter. Atomic: one invalid entry reverts the whole batch (the
    ///      backend validates and sanctions-screens recipients first).
    /// @param recipients Wallets to receive YEET
    /// @param amounts    Amounts in wei (18 decimals)
    /// @param actions    Action labels for event logging
    function batchMintRewards(
        address[] calldata recipients,
        uint256[] calldata amounts,
        string[] calldata actions
    ) external onlyMinter {
        if (mintingPaused) revert MintingIsPaused();
        require(recipients.length == amounts.length, "Length mismatch");
        require(recipients.length == actions.length, "Length mismatch");
        require(recipients.length <= MAX_BATCH, "Max 200 per batch");
        uint256 minted = rewardsMinted;
        uint256 batchTotal;
        for (uint256 i = 0; i < recipients.length; i++) {
            require(recipients[i] != address(0), "Zero recipient");
            minted += amounts[i];
            batchTotal += amounts[i];
            require(minted <= REWARD_RESERVE, "Exceeds reward reserve");
            _mint(recipients[i], amounts[i]);
            emit RewardMinted(recipients[i], amounts[i], actions[i]);
        }
        uint256 day = currentDay();
        uint256 before = mintedOnDay[day];
        uint256 used = before + batchTotal;
        if (used > dailyMintCap) {
            revert DailyCapExceeded(batchTotal, before >= dailyMintCap ? 0 : dailyMintCap - before);
        }
        mintedOnDay[day] = used;
        rewardsMinted = minted;
    }
}
