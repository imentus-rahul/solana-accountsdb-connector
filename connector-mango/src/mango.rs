use {
    async_trait::async_trait,
    bytes::{BufMut, BytesMut},
    fixed::types::I80F48,
    mango::state::{DataType, MangoAccount, MangoCache, MangoGroup},
    mango_common::Loadable,
    postgres_types::{IsNull, ToSql, Type},
    std::{cmp, error, mem},
};

use crate::{encode_address, AccountTable, AccountWrite};

#[derive(Debug, Clone)]
pub struct SqlNumericI80F48(pub I80F48);

impl ToSql for SqlNumericI80F48 {
    fn to_sql(
        &self,
        _: &postgres_types::Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn error::Error + 'static + Sync + Send>> {
        if self.0 == 0 {
            out.reserve(10);
            out.put_u16(1); // num groups
            out.put_i16(0); // first group weight
            out.put_u16(0); // sign
            out.put_u16(0); // dscale
            out.put_i16(0); // first group
            return Ok(IsNull::No);
        }

        let abs_val = self.0.abs();
        let decimals = abs_val.int_log10();
        let first_group_weight = ((decimals as f64) / 4.0f64).floor() as i16;
        let last_group_weight = -4i16;
        let num_groups = (first_group_weight - last_group_weight + 1) as usize;

        // Reserve bytes
        out.reserve(8 + num_groups * 2);

        // Number of groups
        out.put_u16(num_groups as u16);
        // Weight of first group
        out.put_i16(first_group_weight);
        // Sign
        out.put_u16(if self.0 < 0 { 0x4000 } else { 0x0000 });
        // DScale
        out.put_u16(16);

        let mut int_part = abs_val.int().to_num::<u128>();
        let mut frac_part = (abs_val.frac() * I80F48::from_num(1e16)).to_num::<u64>();

        //info!("i80f48 {} {} {} {} {}", self.0, decimals, first_group_weight, int_part, frac_part);

        for weight in (0..=first_group_weight).rev() {
            let decimal_shift = 10000u128.pow(weight as u32);
            let v = (int_part / decimal_shift) & 0xFFFF;
            out.put_i16(v as i16);
            //info!("int {} {} {}", weight, v, int_part);
            int_part -= v * decimal_shift;
        }
        for weight in (last_group_weight..=cmp::min(first_group_weight, -1)).rev() {
            let decimal_shift = 10000u64.pow((4 + weight) as u32);
            let v = (frac_part / decimal_shift) & 0xFFFF;
            out.put_i16(v as i16);
            //info!("frac {} {} {}", weight, v, frac_part);
            frac_part -= v * decimal_shift;
        }

        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        matches!(*ty, Type::NUMERIC)
    }

    postgres_types::to_sql_checked!();
}

// from https://github.com/rust-lang/rust/pull/86930
mod int_log {
    // 0 < val < 100_000_000
    const fn less_than_8(mut val: u32) -> u32 {
        let mut log = 0;
        if val >= 10_000 {
            val /= 10_000;
            log += 4;
        }
        log + if val >= 1000 {
            3
        } else if val >= 100 {
            2
        } else if val >= 10 {
            1
        } else {
            0
        }
    }

    // 0 < val < 10_000_000_000_000_000
    const fn less_than_16(mut val: u64) -> u32 {
        let mut log = 0;
        if val >= 100_000_000 {
            val /= 100_000_000;
            log += 8;
        }
        log + less_than_8(val as u32)
    }

    // 0 < val <= u64::MAX
    pub const fn u64(mut val: u64) -> u32 {
        let mut log = 0;
        if val >= 10_000_000_000_000_000 {
            val /= 10_000_000_000_000_000;
            log += 16;
        }
        log + less_than_16(val)
    }

    // 0 < val <= u128::MAX
    pub const fn u128(mut val: u128) -> u32 {
        let mut log = 0;
        if val >= 100_000_000_000_000_000_000_000_000_000_000 {
            val /= 100_000_000_000_000_000_000_000_000_000_000;
            log += 32;
            return log + less_than_8(val as u32);
        }
        if val >= 10_000_000_000_000_000 {
            val /= 10_000_000_000_000_000;
            log += 16;
        }
        log + less_than_16(val as u64)
    }
}

#[derive(Debug, Clone)]
pub struct SqlNumericI128(pub i128);

impl ToSql for SqlNumericI128 {
    fn to_sql(
        &self,
        _: &postgres_types::Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn error::Error + 'static + Sync + Send>> {
        let abs_val = self.0.abs() as u128;
        let decimals = if self.0 != 0 {
            int_log::u128(abs_val)
        } else {
            0
        };
        let first_group_weight = ((decimals as f64) / 4.0f64).floor() as i16;
        let num_groups = (first_group_weight + 1) as usize;

        // Reserve bytes
        out.reserve(8 + num_groups * 2);

        // Number of groups
        out.put_u16(num_groups as u16);
        // Weight of first group
        out.put_i16(first_group_weight);
        // Sign
        out.put_u16(if self.0 < 0 { 0x4000 } else { 0x0000 });
        // DScale
        out.put_u16(0);

        let mut int_part = abs_val;

        for weight in (0..=first_group_weight).rev() {
            let decimal_shift = 10000u128.pow(weight as u32);
            let v = (int_part / decimal_shift) & 0xFFFF;
            out.put_i16(v as i16);
            int_part -= v * decimal_shift;
        }

        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        matches!(*ty, Type::NUMERIC)
    }

    postgres_types::to_sql_checked!();
}

#[derive(Debug, Clone)]
pub struct SqlNumericU64(pub u64);

impl ToSql for SqlNumericU64 {
    fn to_sql(
        &self,
        _: &postgres_types::Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn error::Error + 'static + Sync + Send>> {
        let decimals = if self.0 != 0 { int_log::u64(self.0) } else { 0 };
        let first_group_weight = ((decimals as f64) / 4.0f64).floor() as i16;
        let num_groups = (first_group_weight + 1) as usize;

        // Reserve bytes
        out.reserve(8 + num_groups * 2);

        // Number of groups
        out.put_u16(num_groups as u16);
        // Weight of first group
        out.put_i16(first_group_weight);
        // Sign
        out.put_u16(0);
        // DScale
        out.put_u16(0);

        let mut int_part = self.0;

        for weight in (0..=first_group_weight).rev() {
            let decimal_shift = 10000u64.pow(weight as u32);
            let v = (int_part / decimal_shift) & 0xFFFF;
            out.put_i16(v as i16);
            int_part -= v * decimal_shift;
        }

        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        matches!(*ty, Type::NUMERIC)
    }

    postgres_types::to_sql_checked!();
}

#[derive(Debug, ToSql)]
struct PerpAccount {
    base_position: i64,
    quote_position: SqlNumericI80F48,
    long_settled_funding: SqlNumericI80F48,
    short_settled_funding: SqlNumericI80F48,
    bids_quantity: i64,
    asks_quantity: i64,
    taker_base: i64,
    taker_quote: i64,
    mngo_accrued: SqlNumericU64,
}

pub struct MangoAccountTable {}

#[async_trait]
impl AccountTable for MangoAccountTable {
    fn table_name(&self) -> &str {
        "mango_account_write"
    }

    async fn insert_account_write(
        &self,
        client: &postgres_query::Caching<tokio_postgres::Client>,
        account_write: &AccountWrite,
    ) -> anyhow::Result<()> {
        if account_write.data.len() != mem::size_of::<MangoAccount>()
            || account_write.data[0] != DataType::MangoAccount as u8
        {
            return Ok(());
        }

        // TODO: Also filter on mango_group?

        let pubkey = encode_address(&account_write.pubkey);
        let data = MangoAccount::load_from_bytes(&account_write.data)?;

        let owner = encode_address(&data.owner);
        let mango_group = encode_address(&data.mango_group);
        let version = data.meta_data.version as i16;
        let extra_info = &data.meta_data.extra_info as &[u8];
        let in_margin_basket = &data.in_margin_basket as &[bool];
        let num_in_margin_basket = data.num_in_margin_basket as i16;
        let deposits = data
            .deposits
            .iter()
            .map(|v| SqlNumericI80F48(*v))
            .collect::<Vec<SqlNumericI80F48>>();
        let borrows = data
            .borrows
            .iter()
            .map(|v| SqlNumericI80F48(*v))
            .collect::<Vec<SqlNumericI80F48>>();
        let spot_open_orders = data
            .spot_open_orders
            .iter()
            .map(|key| encode_address(&key))
            .collect::<Vec<String>>();
        let perp_accounts = data
            .perp_accounts
            .iter()
            .map(|perp| PerpAccount {
                base_position: perp.base_position,
                quote_position: SqlNumericI80F48(perp.quote_position),
                long_settled_funding: SqlNumericI80F48(perp.long_settled_funding),
                short_settled_funding: SqlNumericI80F48(perp.short_settled_funding),
                bids_quantity: perp.bids_quantity,
                asks_quantity: perp.asks_quantity,
                taker_base: perp.taker_base,
                taker_quote: perp.taker_quote,
                mngo_accrued: SqlNumericU64(perp.mngo_accrued),
            })
            .collect::<Vec<PerpAccount>>();
        let order_market = data
            .order_market
            .iter()
            .map(|v| *v as i16)
            .collect::<Vec<i16>>();
        let order_side = data
            .order_side
            .iter()
            .map(|v| *v as i16)
            .collect::<Vec<i16>>();
        let orders = data
            .orders
            .iter()
            .map(|v| SqlNumericI128(*v))
            .collect::<Vec<SqlNumericI128>>();
        let client_order_ids = data
            .client_order_ids
            .iter()
            .map(|v| SqlNumericU64(*v))
            .collect::<Vec<SqlNumericU64>>();
        let msrm_amount = SqlNumericU64(data.msrm_amount);
        let info = &data.info as &[u8];
        let advanced_orders_key = encode_address(&data.advanced_orders_key);
        let padding = &data.padding as &[u8];

        let query = postgres_query::query!(
            "
            INSERT INTO mango_account_write
            (pubkey_id, slot, write_version,
            version, is_initialized, extra_info, mango_group_id,
            owner_id, in_margin_basket, num_in_margin_basket, deposits,
            borrows, spot_open_orders_ids, perp_accounts, order_market,
            order_side, orders, client_order_ids,
            msrm_amount, being_liquidated, is_bankrupt, info,
            advanced_orders_key_id, padding
            )
            VALUES
            (map_pubkey($pubkey), $slot, $write_version,
            $version, $is_initialized, $extra_info, map_pubkey($mango_group),
            map_pubkey($owner), $in_margin_basket, $num_in_margin_basket, $deposits,
            $borrows, map_pubkey_arr($spot_open_orders), $perp_accounts, $order_market,
            $order_side, $orders, $client_order_ids,
            $msrm_amount, $being_liquidated, $is_bankrupt, $info,
            map_pubkey($advanced_orders_key), $padding
            )
            ON CONFLICT (pubkey_id, slot, write_version) DO NOTHING",
            pubkey,
            slot = account_write.slot,
            write_version = account_write.write_version,
            version,
            is_initialized = data.meta_data.is_initialized,
            extra_info,
            mango_group,
            owner,
            in_margin_basket,
            num_in_margin_basket,
            deposits,
            borrows,
            spot_open_orders,
            perp_accounts,
            order_market,
            order_side,
            orders,
            client_order_ids,
            msrm_amount,
            being_liquidated = data.being_liquidated,
            is_bankrupt = data.is_bankrupt,
            info,
            advanced_orders_key,
            padding,
        );
        let _ = query.execute(client).await?;
        Ok(())
    }
}

#[derive(Debug, ToSql)]
struct TokenInfo {
    mint: String,
    root_bank: String,
    decimals: i16,
    padding: Vec<u8>,
}
#[derive(Debug, ToSql)]
struct SpotMarketInfo {
    spot_market: String,
    maint_asset_weight: SqlNumericI80F48,
    init_asset_weight: SqlNumericI80F48,
    maint_liab_weight: SqlNumericI80F48,
    init_liab_weight: SqlNumericI80F48,
    liquidation_fee: SqlNumericI80F48,
}
#[derive(Debug, ToSql)]
struct PerpMarketInfo {
    perp_market: String,
    maint_asset_weight: SqlNumericI80F48,
    init_asset_weight: SqlNumericI80F48,
    maint_liab_weight: SqlNumericI80F48,
    init_liab_weight: SqlNumericI80F48,
    liquidation_fee: SqlNumericI80F48,
    maker_fee: SqlNumericI80F48,
    taker_fee: SqlNumericI80F48,
    base_lot_size: i64,
    quote_lot_size: i64,
}

pub struct MangoGroupTable {}

#[async_trait]
impl AccountTable for MangoGroupTable {
    fn table_name(&self) -> &str {
        "mango_group_write"
    }

    async fn insert_account_write(
        &self,
        client: &postgres_query::Caching<tokio_postgres::Client>,
        account_write: &AccountWrite,
    ) -> anyhow::Result<()> {
        if account_write.data.len() != mem::size_of::<MangoGroup>()
            || account_write.data[0] != DataType::MangoGroup as u8
        {
            return Ok(());
        }

        // TODO: Also filter on mango_group pubkey?

        let pubkey = encode_address(&account_write.pubkey);
        let data = MangoGroup::load_from_bytes(&account_write.data)?;
        let version = data.meta_data.version as i16;
        let extra_info = &data.meta_data.extra_info as &[u8];
        let num_oracles = data.num_oracles as i64;
        let tokens = data
            .tokens
            .iter()
            .map(|token| TokenInfo {
                mint: encode_address(&token.mint),
                root_bank: encode_address(&token.root_bank),
                decimals: token.decimals as i16,
                padding: token.padding.to_vec(),
            })
            .collect::<Vec<TokenInfo>>();
        let spot_markets = data
            .spot_markets
            .iter()
            .map(|market| SpotMarketInfo {
                spot_market: encode_address(&market.spot_market),
                maint_asset_weight: SqlNumericI80F48(market.maint_asset_weight),
                init_asset_weight: SqlNumericI80F48(market.init_asset_weight),
                maint_liab_weight: SqlNumericI80F48(market.maint_liab_weight),
                init_liab_weight: SqlNumericI80F48(market.init_liab_weight),
                liquidation_fee: SqlNumericI80F48(market.liquidation_fee),
            })
            .collect::<Vec<SpotMarketInfo>>();
        let perp_markets = data
            .perp_markets
            .iter()
            .map(|market| PerpMarketInfo {
                perp_market: encode_address(&market.perp_market),
                maint_asset_weight: SqlNumericI80F48(market.maint_asset_weight),
                init_asset_weight: SqlNumericI80F48(market.init_asset_weight),
                maint_liab_weight: SqlNumericI80F48(market.maint_liab_weight),
                init_liab_weight: SqlNumericI80F48(market.init_liab_weight),
                liquidation_fee: SqlNumericI80F48(market.liquidation_fee),
                maker_fee: SqlNumericI80F48(market.maker_fee),
                taker_fee: SqlNumericI80F48(market.taker_fee),
                base_lot_size: market.base_lot_size,
                quote_lot_size: market.quote_lot_size,
            })
            .collect::<Vec<PerpMarketInfo>>();
        let oracles = data
            .oracles
            .iter()
            .map(|key| encode_address(&key))
            .collect::<Vec<String>>();
        let signer_nonce = SqlNumericU64(data.signer_nonce);
        let signer_key = encode_address(&data.signer_key);
        let admin = encode_address(&data.admin);
        let dex_program_id = encode_address(&data.dex_program_id);
        let mango_cache = encode_address(&data.mango_cache);
        let valid_interval = SqlNumericU64(data.valid_interval);
        let insurance_vault = encode_address(&data.insurance_vault);
        let srm_vault = encode_address(&data.srm_vault);
        let msrm_vault = encode_address(&data.msrm_vault);
        let fees_vault = encode_address(&data.fees_vault);
        let padding = &data.padding as &[u8];

        let query = postgres_query::query!(
            "
            INSERT INTO mango_group_write
            (pubkey_id, slot, write_version,
            version, is_initialized, extra_info,
            num_oracles,
            tokens,
            spot_markets,
            perp_markets,
            oracle_ids, signer_nonce, signer_key_id, admin_id,
            dex_program_id, mango_cache_id, valid_interval,
            insurance_vault_id, srm_vault_id, msrm_vault_id,
            fees_vault_id,
            padding)
            VALUES
            (map_pubkey($pubkey), $slot, $write_version,
            $version, $is_initialized, $extra_info,
            $num_oracles,
            $tokens,
            $spot_markets,
            $perp_markets,
            map_pubkey_arr($oracles), $signer_nonce, map_pubkey($signer_key), map_pubkey($admin),
            map_pubkey($dex_program_id), map_pubkey($mango_cache), $valid_interval,
            map_pubkey($insurance_vault), map_pubkey($srm_vault), map_pubkey($msrm_vault),
            map_pubkey($fees_vault),
            $padding)
            ON CONFLICT (pubkey_id, slot, write_version) DO NOTHING",
            pubkey,
            slot = account_write.slot,
            write_version = account_write.write_version,
            version,
            is_initialized = data.meta_data.is_initialized,
            extra_info,
            num_oracles,
            tokens,
            spot_markets,
            perp_markets,
            oracles,
            signer_nonce,
            signer_key,
            admin,
            dex_program_id,
            mango_cache,
            valid_interval,
            insurance_vault,
            srm_vault,
            msrm_vault,
            fees_vault,
            padding,
        );
        let _ = query.execute(client).await?;
        Ok(())
    }
}

#[derive(Debug, ToSql)]
struct PriceCache {
    price: SqlNumericI80F48,
    last_update: SqlNumericU64,
}
#[derive(Debug, ToSql)]
struct RootBankCache {
    deposit_index: SqlNumericI80F48,
    borrow_index: SqlNumericI80F48,
    last_update: SqlNumericU64,
}
#[derive(Debug, ToSql)]
struct PerpMarketCache {
    long_funding: SqlNumericI80F48,
    short_funding: SqlNumericI80F48,
    last_update: SqlNumericU64,
}

pub struct MangoCacheTable {}

#[async_trait]
impl AccountTable for MangoCacheTable {
    fn table_name(&self) -> &str {
        "mango_cache_write"
    }

    async fn insert_account_write(
        &self,
        client: &postgres_query::Caching<tokio_postgres::Client>,
        account_write: &AccountWrite,
    ) -> anyhow::Result<()> {
        if account_write.data.len() != mem::size_of::<MangoCache>()
            || account_write.data[0] != DataType::MangoCache as u8
        {
            return Ok(());
        }

        // TODO: This one can't be fitlered to only use the one for our mango_group?

        let pubkey = encode_address(&account_write.pubkey);
        let data = MangoCache::load_from_bytes(&account_write.data)?;
        let version = data.meta_data.version as i16;
        let extra_info = &data.meta_data.extra_info as &[u8];
        let price_cache = data
            .price_cache
            .iter()
            .map(|cache| PriceCache {
                price: SqlNumericI80F48(cache.price),
                last_update: SqlNumericU64(cache.last_update),
            })
            .collect::<Vec<PriceCache>>();
        let root_bank_cache = data
            .root_bank_cache
            .iter()
            .map(|cache| RootBankCache {
                deposit_index: SqlNumericI80F48(cache.deposit_index),
                borrow_index: SqlNumericI80F48(cache.borrow_index),
                last_update: SqlNumericU64(cache.last_update),
            })
            .collect::<Vec<RootBankCache>>();
        let perp_market_cache = data
            .perp_market_cache
            .iter()
            .map(|cache| PerpMarketCache {
                long_funding: SqlNumericI80F48(cache.long_funding),
                short_funding: SqlNumericI80F48(cache.short_funding),
                last_update: SqlNumericU64(cache.last_update),
            })
            .collect::<Vec<PerpMarketCache>>();

        let query = postgres_query::query!(
            "
            INSERT INTO mango_cache_write
            (pubkey_id, slot, write_version,
            version, is_initialized, extra_info,
            price_cache, root_bank_cache, perp_market_cache)
            VALUES
            (map_pubkey($pubkey), $slot, $write_version,
            $version, $is_initialized, $extra_info,
            $price_cache, $root_bank_cache, $perp_market_cache)
            ON CONFLICT (pubkey_id, slot, write_version) DO NOTHING",
            pubkey,
            slot = account_write.slot,
            write_version = account_write.write_version,
            version,
            is_initialized = data.meta_data.is_initialized,
            extra_info,
            price_cache,
            root_bank_cache,
            perp_market_cache,
        );
        let _ = query.execute(client).await?;
        Ok(())
    }
}
