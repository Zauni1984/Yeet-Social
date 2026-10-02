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
/// The reward tranche is the only thing that can ever be minted after deployment, it is
/// bounded by `REWARD_RESERVE` independently of burns, and there is no other mint function —
/// so `totalSupply()` can never exceed `MAX_SUPPLY`. The backend's conversion pool
/// (`YEET_CONVERSION_POOL`, default = `REWARD_RESERVE`) mirrors this number off-chain.
///
/// Ownership uses `Ownable2Step` so the hand-over to a multisig (docs/mica/04 §B2) cannot
/// be sent to a wrong address by accident.
contract YeetToken is ERC20, ERC20Burnable, ERC20Permit, Ownable2Step {
    uint256 public constant MAX_SUPPLY      = 21_000_000_000 * 10 ** 18;
    uint256 public constant DEVELOPER_SHARE = (MAX_SUPPLY * 10) / 100;
    uint256 public constant TEAM_SHARE      = (MAX_SUPPLY * 10) / 100;
    uint256 public constant RESERVE_SHARE   = (MAX_SUPPLY * 5) / 100;
    /// @notice The 75 % rewards/community tranche: the only mintable amount after deploy.
    uint256 public constant REWARD_RESERVE  = MAX_SUPPLY - DEVELOPER_SHARE - TEAM_SHARE - RESERVE_SHARE;
    /// @notice Maximum recipients per `batchMintRewards` call (gas bound).
    uint256 public constant MAX_BATCH = 200;

    /// @notice Cumulative YEET minted from the reward reserve (monotonic; burns do not lower it).
    uint256 public rewardsMinted;

    event RewardMinted(address indexed recipient, uint256 amount, string action);

    constructor(address initialOwner)
        ERC20("Yeet Token", "YEET")
        ERC20Permit("Yeet Token")
        Ownable(initialOwner)
    {
        // Developer + Team + Reserve (25 %). Vesting for Developer/Team is handled by
        // separate vesting contracts/multisig policy (docs/mica/02), not by this token.
        _mint(initialOwner, DEVELOPER_SHARE + TEAM_SHARE + RESERVE_SHARE);
    }

    /// @notice Decimals override (18, same as ETH)
    function decimals() public pure override returns (uint8) {
        return 18;
    }

    /// @notice YEET still mintable from the reward reserve.
    function rewardsRemaining() external view returns (uint256) {
        return REWARD_RESERVE - rewardsMinted;
    }

    /// @notice Mint reward-tranche YEET to multiple addresses (points→YEET conversions).
    /// @dev Only the owner (reward minter / multisig). Atomic: one invalid entry reverts
    ///      the whole batch (the backend validates and sanctions-screens recipients first).
    /// @param recipients Wallets to receive YEET
    /// @param amounts    Amounts in wei (18 decimals)
    /// @param actions    Action labels for event logging
    function batchMintRewards(
        address[] calldata recipients,
        uint256[] calldata amounts,
        string[] calldata actions
    ) external onlyOwner {
        require(recipients.length == amounts.length, "Length mismatch");
        require(recipients.length == actions.length, "Length mismatch");
        require(recipients.length <= MAX_BATCH, "Max 200 per batch");
        uint256 minted = rewardsMinted;
        for (uint256 i = 0; i < recipients.length; i++) {
            minted += amounts[i];
            require(minted <= REWARD_RESERVE, "Exceeds reward reserve");
            _mint(recipients[i], amounts[i]);
            emit RewardMinted(recipients[i], amounts[i], actions[i]);
        }
        rewardsMinted = minted;
    }
}
