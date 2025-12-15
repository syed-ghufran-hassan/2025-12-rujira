use cosmwasm_std::StdError;
use cw_storage_plus::{Key, KeyDeserialize, Prefixer, PrimaryKey};
use rujira_rs::fin::{Price, Side};

// Provided as a type to prefix on price type, without duplicating in the key
#[derive(Clone, Debug, PartialEq)]
pub enum PoolType {
    Fixed,
    Oracle,
}

impl PrimaryKey<'_> for PoolType {
    type Prefix = ();
    type SubPrefix = ();
    type Suffix = ();
    type SuperSuffix = ();

    fn key(&self) -> std::vec::Vec<Key<'_>> {
        match self {
            PoolType::Fixed => vec![Key::Val8([0])],
            PoolType::Oracle => vec![Key::Val8([1])],
        }
    }
}

impl<'a> Prefixer<'a> for PoolType {
    fn prefix(&self) -> Vec<Key> {
        self.key()
    }
}

impl KeyDeserialize for PoolType {
    type Output = Self;
    const KEY_ELEMS: u16 = 1;

    fn from_vec(value: Vec<u8>) -> cosmwasm_std::StdResult<Self::Output> {
        match value.first() {
            Some(0u8) => Ok(Self::Fixed),
            Some(1u8) => Ok(Self::Oracle),
            _ => Err(StdError::generic_err("invalid PoolType key")),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PoolKey {
    pub side: Side,
    pub price: Price,
}

impl PoolKey {
    pub fn new(side: Side, price: Price) -> Self {
        Self { side, price }
    }
}

impl<'a> PrimaryKey<'a> for PoolKey {
    type Prefix = (Side, PoolType);
    type SubPrefix = Side;
    type Suffix = Price;
    type SuperSuffix = (PoolType, Price);

    fn key(&self) -> std::vec::Vec<Key<'_>> {
        let mut key = self.side.key();

        match self.price {
            Price::Fixed(_) => {
                key.extend(PoolType::Fixed.key());
            }
            Price::Oracle(_) => {
                key.extend(PoolType::Oracle.key());
            }
        };
        key.extend(self.price.key());
        key
    }
}

impl<'a> Prefixer<'a> for PoolKey {
    fn prefix(&self) -> Vec<Key> {
        self.key()
    }
}

impl KeyDeserialize for PoolKey {
    type Output = Self;
    const KEY_ELEMS: u16 = 3;

    fn from_vec(value: Vec<u8>) -> cosmwasm_std::StdResult<Self::Output> {
        // 2 bytes namespace length
        let side = <Side>::from_vec(value[2..3].to_vec())?;
        // 2 more
        let price = <Price>::from_vec(value[6..].to_vec())?;
        Ok(Self { side, price })
    }
}

#[cfg(test)]

mod tests {
    use super::*;
    use cosmwasm_std::Decimal;
    use rujira_rs::fin::{Price, Side};
    use std::str::FromStr;

    #[test]
    fn pool_key() {
        let key = PoolKey::new(Side::Base, Price::Fixed(Decimal::from_str("1.0").unwrap()));
        let key_bytes = key.key();

        assert_eq!(key_bytes.len(), 3);
        assert_eq!(key_bytes[0].as_ref(), &[0]);
        assert_eq!(key_bytes[1].as_ref(), &[0]);
        assert_eq!(
            key_bytes[2].as_ref(),
            &[0, 0, 0, 0, 0, 0, 0, 0, 13, 224, 182, 179, 167, 100, 0, 0]
        );

        let key = PoolKey::new(Side::Quote, Price::Oracle(-15));
        let key_bytes = key.key();

        assert_eq!(key_bytes[0].as_ref(), &[1]);
        assert_eq!(key_bytes[1].as_ref(), &[1]);
        assert_eq!(key_bytes[2].as_ref(), &[127, 241]);

        let key = PoolKey::new(Side::Quote, Price::Oracle(21));
        let key_bytes = key.key();

        assert_eq!(key_bytes[0].as_ref(), &[1]);
        assert_eq!(key_bytes[1].as_ref(), &[1]);
        assert_eq!(key_bytes[2].as_ref(), &[128, 21]);

        let key = PoolKey::new(Side::Quote, Price::Oracle(0));
        let key_bytes = key.key();

        assert_eq!(key_bytes[0].as_ref(), &[1]);
        assert_eq!(key_bytes[1].as_ref(), &[1]);
        assert_eq!(key_bytes[2].as_ref(), &[128, 0]);

        let key = PoolKey::new(Side::Quote, Price::Oracle(0));
        assert_eq!(<PoolKey>::from_slice(&key.joined_key()).unwrap(), key);

        let key = PoolKey::new(Side::Base, Price::Oracle(0));
        assert_eq!(<PoolKey>::from_slice(&key.joined_key()).unwrap(), key);

        let key = PoolKey::new(Side::Base, Price::Fixed(Decimal::one()));
        assert_eq!(<PoolKey>::from_slice(&key.joined_key()).unwrap(), key);

        let key = PoolKey::new(Side::Quote, Price::Fixed(Decimal::one()));
        assert_eq!(<PoolKey>::from_slice(&key.joined_key()).unwrap(), key);
    }
}
