//! This file contains all of the code that is used to load and validate account
//! data. Each loader struct describes specific account types and constraints
//! that must be met for the instruction data to be valid. Each AccountInfo is checked
//! according to a particular checker struct and if the account data is invalid, an error is
//! returned and the instruction will fail.
//!
//! The loader structs are used to validate the accounts in the instruction data

use super::checkers::{MarketAccountInfo, MintAccountInfo, SeatAccountInfo, TokenAccountInfo, PDA};
use crate::{
    phoenix_log_authority,
    program::{
        validation::checkers::{EmptyAccount, Program, Signer},
        TokenParams,
    },
};
use core::slice::Iter;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    program_error::ProgramError,
    pubkey::Pubkey,
    system_program,
};

pub fn get_vault_address(market: &Pubkey, mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"vault", market.as_ref(), mint.as_ref()], &crate::ID)
}

pub fn get_seat_address(market: &Pubkey, trader: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"seat", market.as_ref(), trader.as_ref()], &crate::ID)
}

pub struct PhoenixLogContext<'a, 'info> {
    pub phoenix_program: Program<'a, 'info>,
    pub log_authority: PDA<'a, 'info>,
}

impl<'a, 'info> PhoenixLogContext<'a, 'info> {
    pub fn load(account_iter: &mut Iter<'a, AccountInfo<'info>>) -> Result<Self, ProgramError> {
        Ok(Self {
            phoenix_program: Program::new(next_account_info(account_iter)?, &crate::id())?,
            log_authority: PDA::new(
                next_account_info(account_iter)?,
                &phoenix_log_authority::id(),
            )?,
        })
    }
}

pub struct PhoenixMarketContext<'a, 'info> {
    pub market_info: MarketAccountInfo<'a, 'info>,
    pub signer: Signer<'a, 'info>,
}

impl<'a, 'info> PhoenixMarketContext<'a, 'info> {
    pub fn load(account_iter: &mut Iter<'a, AccountInfo<'info>>) -> Result<Self, ProgramError> {
        Ok(Self {
            market_info: MarketAccountInfo::new(next_account_info(account_iter)?)?,
            signer: Signer::new(next_account_info(account_iter)?)?,
        })
    }

    pub fn load_init(
        account_iter: &mut Iter<'a, AccountInfo<'info>>,
    ) -> Result<Self, ProgramError> {
        Ok(Self {
            market_info: MarketAccountInfo::new_init(next_account_info(account_iter)?)?,
            signer: Signer::new(next_account_info(account_iter)?)?,
        })
    }
}

/// These accounts that are required for all market actions that interact with a token vault
pub struct PhoenixVaultContext<'a, 'info> {
    pub base_account: TokenAccountInfo<'a, 'info>,
    pub quote_account: TokenAccountInfo<'a, 'info>,
    pub base_vault: TokenAccountInfo<'a, 'info>,
    pub quote_vault: TokenAccountInfo<'a, 'info>,
    pub token_program: Program<'a, 'info>,
}

impl<'a, 'info> PhoenixVaultContext<'a, 'info> {
    pub fn load_from_iter(
        account_iter: &mut Iter<'a, AccountInfo<'info>>,
        base_params: &TokenParams,
        quote_params: &TokenParams,
        trader_key: &Pubkey,
    ) -> Result<Self, ProgramError> {
        Ok(Self {
            base_account: TokenAccountInfo::new_with_owner(
                next_account_info(account_iter)?,
                &base_params.mint_key,
                trader_key,
            )?,
            quote_account: TokenAccountInfo::new_with_owner(
                next_account_info(account_iter)?,
                &quote_params.mint_key,
                trader_key,
            )?,
            base_vault: TokenAccountInfo::new_with_owner_and_key(
                next_account_info(account_iter)?,
                &base_params.mint_key,
                &base_params.vault_key,
                &base_params.vault_key,
            )?,
            quote_vault: TokenAccountInfo::new_with_owner_and_key(
                next_account_info(account_iter)?,
                &quote_params.mint_key,
                &quote_params.vault_key,
                &quote_params.vault_key,
            )?,
            token_program: Program::new(next_account_info(account_iter)?, &spl_token::id())?,
        })
    }
}

pub struct InitializeMarketContext<'a, 'info> {
    pub base_mint: MintAccountInfo<'a, 'info>,
    pub quote_mint: MintAccountInfo<'a, 'info>,
    pub base_vault: EmptyAccount<'a, 'info>,
    pub quote_vault: EmptyAccount<'a, 'info>,
    pub system_program: Program<'a, 'info>,
    pub token_program: Program<'a, 'info>,
}

impl<'a, 'info> InitializeMarketContext<'a, 'info> {
    pub fn load(accounts: &'a [AccountInfo<'info>]) -> Result<Self, ProgramError> {
        let account_iter = &mut accounts.iter();
        let ctx = Self {
            base_mint: MintAccountInfo::new(next_account_info(account_iter)?)?,
            quote_mint: MintAccountInfo::new(next_account_info(account_iter)?)?,
            base_vault: EmptyAccount::new(next_account_info(account_iter)?)?,
            quote_vault: EmptyAccount::new(next_account_info(account_iter)?)?,
            system_program: Program::new(next_account_info(account_iter)?, &system_program::id())?,
            token_program: Program::new(next_account_info(account_iter)?, &spl_token::id())?,
        };
        Ok(ctx)
    }
}

pub struct NewOrderContext<'a, 'info> {
    // This is only used for limit order instructions
    pub seat_option: Option<SeatAccountInfo<'a, 'info>>,
    pub vault_context: Option<PhoenixVaultContext<'a, 'info>>,
}

impl<'a, 'info> NewOrderContext<'a, 'info> {
    pub fn load_post_allowed(
        market_context: &PhoenixMarketContext<'a, 'info>,
        accounts: &'a [AccountInfo<'info>],
        only_free_funds: bool,
    ) -> Result<Self, ProgramError> {
        let PhoenixMarketContext {
            market_info,
            signer: trader,
        } = market_context;
        market_info.assert_post_allowed()?;
        let account_iter = &mut accounts.iter();
        let seat_option = Some(SeatAccountInfo::new_with_context(
            next_account_info(account_iter)?,
            market_info.key,
            trader.key,
            true,
        )?);
        let new_order_token_account_ctx = if only_free_funds {
            None
        } else {
            let (base_params, quote_params) = {
                let header = market_info.get_header()?;
                (header.base_params, header.quote_params)
            };
            Some(PhoenixVaultContext::load_from_iter(
                account_iter,
                &base_params,
                &quote_params,
                trader.key,
            )?)
        };
        Ok(Self {
            seat_option,
            vault_context: new_order_token_account_ctx,
        })
    }

    pub fn load_cross_only(
        market_context: &PhoenixMarketContext<'a, 'info>,
        accounts: &'a [AccountInfo<'info>],
        only_free_funds: bool,
    ) -> Result<Self, ProgramError> {
        let PhoenixMarketContext {
            market_info,
            signer: trader,
        } = market_context;
        market_info.assert_cross_allowed()?;
        let account_iter = &mut accounts.iter();
        let seat_option = if only_free_funds {
            Some(SeatAccountInfo::new_with_context(
                next_account_info(account_iter)?,
                market_info.key,
                trader.key,
                true,
            )?)
        } else {
            None
        };
        let new_order_token_account_ctx = if only_free_funds {
            None
        } else {
            let (base_params, quote_params) = {
                let header = market_info.get_header()?;
                (header.base_params, header.quote_params)
            };
            Some(PhoenixVaultContext::load_from_iter(
                account_iter,
                &base_params,
                &quote_params,
                trader.key,
            )?)
        };
        Ok(Self {
            seat_option,
            vault_context: new_order_token_account_ctx,
        })
    }
}

pub struct CancelOrWithdrawContext<'a, 'info> {
    pub vault_context: PhoenixVaultContext<'a, 'info>,
}

impl<'a, 'info> CancelOrWithdrawContext<'a, 'info> {
    pub fn load(
        market_context: &PhoenixMarketContext<'a, 'info>,
        accounts: &'a [AccountInfo<'info>],
    ) -> Result<Self, ProgramError> {
        let PhoenixMarketContext {
            market_info,
            signer: trader,
        } = market_context;
        market_info.assert_reduce_allowed()?;
        let account_iter = &mut accounts.iter();
        let trader_key = trader.key;
        let (base_params, quote_params) = {
            let header = market_info.get_header()?;
            (header.base_params, header.quote_params)
        };
        let ctx = Self {
            vault_context: PhoenixVaultContext::load_from_iter(
                account_iter,
                &base_params,
                &quote_params,
                trader_key,
            )?,
        };
        Ok(ctx)
    }
}

pub struct DepositContext<'a, 'info> {
    pub seat: SeatAccountInfo<'a, 'info>,
    pub vault_context: PhoenixVaultContext<'a, 'info>,
}

impl<'a, 'info> DepositContext<'a, 'info> {
    pub fn load(
        market_context: &PhoenixMarketContext<'a, 'info>,
        accounts: &'a [AccountInfo<'info>],
    ) -> Result<Self, ProgramError> {
        let PhoenixMarketContext {
            market_info,
            signer: trader,
        } = market_context;
        market_info.assert_post_allowed()?;
        let account_iter = &mut accounts.iter();
        let market_key = market_info.key;
        let trader_key = trader.key;
        let (base_params, quote_params) = {
            let header = market_info.get_header()?;
            (header.base_params, header.quote_params)
        };
        let ctx = Self {
            seat: SeatAccountInfo::new_with_context(
                next_account_info(account_iter)?,
                market_key,
                trader_key,
                true,
            )?,
            vault_context: PhoenixVaultContext::load_from_iter(
                account_iter,
                &base_params,
                &quote_params,
                trader_key,
            )?,
        };
        Ok(ctx)
    }
}

pub struct AuthorizedActionContext<'a, 'info> {
    pub trader: &'a AccountInfo<'info>,
    pub seat: SeatAccountInfo<'a, 'info>,
    pub vault_context: PhoenixVaultContext<'a, 'info>,
}

impl<'a, 'info> AuthorizedActionContext<'a, 'info> {
    pub fn load(
        market_context: &PhoenixMarketContext<'a, 'info>,
        accounts: &'a [AccountInfo<'info>],
    ) -> Result<Self, ProgramError> {
        let PhoenixMarketContext {
            market_info,
            signer: authority,
        } = market_context;
        market_info.assert_valid_authority(authority.key)?;
        let (base_params, quote_params) = {
            let header = market_info.get_header()?;
            (header.base_params, header.quote_params)
        };
        let market_key = *market_info.key;

        let account_iter = &mut accounts.iter();
        let trader_info = next_account_info(account_iter)?;

        let ctx = Self {
            trader: trader_info,
            seat: SeatAccountInfo::new_with_context(
                next_account_info(account_iter)?,
                &market_key,
                trader_info.key,
                false,
            )?,
            vault_context: PhoenixVaultContext::load_from_iter(
                account_iter,
                &base_params,
                &quote_params,
                trader_info.key,
            )?,
        };

        Ok(ctx)
    }
}

pub struct ChangeMarketStatusContext<'a, 'info> {
    pub receiver: Option<&'a AccountInfo<'info>>,
}

impl<'a, 'info> ChangeMarketStatusContext<'a, 'info> {
    pub fn load(accounts: &'a [AccountInfo<'info>]) -> Result<Self, ProgramError> {
        let account_iter = &mut accounts.iter();
        let ctx = Self {
            receiver: next_account_info(account_iter).ok(),
        };
        Ok(ctx)
    }
}

pub struct AuthorizedSeatRequestContext<'a, 'info> {
    pub payer: Signer<'a, 'info>,
    pub trader: &'a AccountInfo<'info>,
    pub seat: EmptyAccount<'a, 'info>,
    pub system_program: Program<'a, 'info>,
}

impl<'a, 'info> AuthorizedSeatRequestContext<'a, 'info> {
    pub fn load(
        market_context: &PhoenixMarketContext<'a, 'info>,
        accounts: &'a [AccountInfo<'info>],
    ) -> Result<Self, ProgramError> {
        let PhoenixMarketContext {
            market_info,
            signer: authority,
        } = market_context;
        market_info.assert_valid_authority(authority.key)?;

        let account_iter = &mut accounts.iter();
        let ctx = Self {
            payer: Signer::new_payer(next_account_info(account_iter)?)?,
            trader: next_account_info(account_iter)?,
            seat: EmptyAccount::new(next_account_info(account_iter)?)?,
            system_program: Program::new(next_account_info(account_iter)?, &system_program::id())?,
        };
        Ok(ctx)
    }
}

pub struct RequestSeatContext<'a, 'info> {
    pub seat: EmptyAccount<'a, 'info>,
    pub system_program: Program<'a, 'info>,
}

impl<'a, 'info> RequestSeatContext<'a, 'info> {
    pub fn load(
        market_context: &PhoenixMarketContext<'a, 'info>,
        accounts: &'a [AccountInfo<'info>],
    ) -> Result<Self, ProgramError> {
        let PhoenixMarketContext { market_info, .. } = market_context;
        market_info.assert_post_allowed()?;

        let account_iter = &mut accounts.iter();
        let ctx = Self {
            seat: EmptyAccount::new(next_account_info(account_iter)?)?,
            system_program: Program::new(next_account_info(account_iter)?, &system_program::id())?,
        };
        Ok(ctx)
    }
}

pub struct ModifySeatContext<'a, 'info> {
    pub seat: SeatAccountInfo<'a, 'info>,
}

impl<'a, 'info> ModifySeatContext<'a, 'info> {
    pub fn load(
        market_context: &PhoenixMarketContext<'a, 'info>,
        accounts: &'a [AccountInfo<'info>],
    ) -> Result<Self, ProgramError> {
        let PhoenixMarketContext {
            market_info,
            signer: authority,
        } = market_context;
        market_info.assert_valid_authority(authority.key)?;

        let account_iter = &mut accounts.iter();
        let ctx = Self {
            seat: SeatAccountInfo::new(next_account_info(account_iter)?)?,
        };
        Ok(ctx)
    }
}

pub struct CollectFeesContext<'a, 'info> {
    pub fee_recipient_token_account: TokenAccountInfo<'a, 'info>,
    pub quote_vault: TokenAccountInfo<'a, 'info>,
    pub token_program: Program<'a, 'info>,
}

impl<'a, 'info> CollectFeesContext<'a, 'info> {
    pub fn load(
        market_context: &PhoenixMarketContext<'a, 'info>,
        accounts: &'a [AccountInfo<'info>],
    ) -> Result<Self, ProgramError> {
        let (quote_params, fee_recipient) = {
            let header = market_context.market_info.get_header()?;
            (header.quote_params, header.fee_recipient)
        };
        let account_iter = &mut accounts.iter();
        let ctx = Self {
            fee_recipient_token_account: TokenAccountInfo::new_with_owner(
                next_account_info(account_iter)?,
                &quote_params.mint_key,
                &fee_recipient,
            )?,
            quote_vault: TokenAccountInfo::new_with_owner(
                next_account_info(account_iter)?,
                &quote_params.mint_key,
                &quote_params.vault_key,
            )?,
            token_program: Program::new(next_account_info(account_iter)?, &spl_token::id())?,
        };
        Ok(ctx)
    }
}

pub struct ChangeFeeRecipientContext<'a, 'info> {
    pub new_fee_recipient: AccountInfo<'info>,
    pub previous_fee_recipient: Option<Signer<'a, 'info>>,
}

impl<'a, 'info> ChangeFeeRecipientContext<'a, 'info> {
    pub fn load(
        market_context: &PhoenixMarketContext<'a, 'info>,
        accounts: &'a [AccountInfo<'info>],
    ) -> Result<Self, ProgramError> {
        let PhoenixMarketContext {
            market_info,
            signer: authority,
        } = market_context;
        market_info.assert_valid_authority(authority.key)?;
        let current_fee_recipient = {
            let header = market_info.get_header()?;
            header.fee_recipient
        };
        let account_iter = &mut accounts.iter();
        let ctx = Self {
            new_fee_recipient: next_account_info(account_iter)?.clone(),
            previous_fee_recipient: next_account_info(account_iter)
                .and_then(|a| Signer::new_with_key(a, &current_fee_recipient))
                .ok(),
        };
        Ok(ctx)
    }
}