//! Standalone number theoretic functions
//!
//! The functions in this module can be called without an instance of [crate::traits::PrimeBuffer].
//! However, some functions do internally call the implementation on [PrimeBufferExt]
//! (especially those dependent of integer factorization). For these functions, if you have
//! to call them repeatedly, it's recommended to create a [crate::traits::PrimeBuffer]
//! instance and use its associated methods for better performance.
//!
//! For number theoretic functions that depends on integer factorization, strongest primality
//! check will be used in factorization, since for these functions we prefer correctness
//! over speed.
//!

use crate::buffer::{NaiveBuffer, PrimeBufferExt};
use crate::factor::{pollard_rho, squfof};
use crate::primality::{PrimalityBase, PrimalityRefBase};
use crate::tables::{MOEBIUS_ODD, SMALL_PRIMES, WHEEL_NEXT, WHEEL_PREV, WHEEL_SIZE};
#[cfg(feature = "big-table")]
use crate::tables::{SMALL_PRIMES_INV, SMALL_PRIMES_INVLIM};
use crate::traits::{FactorizationConfig, Primality, PrimalityTestConfig, PrimalityUtils};
use crate::RandPrime;
#[cfg(feature = "num-bigint")]
use num_bigint::{BigUint, RandBigInt};
use num_integer::Roots;
use num_modular::ModularOps;
use num_traits::CheckedAdd;
use num_traits::{FromPrimitive, ToPrimitive};
use rand::{random, Rng};
use std::collections::BTreeMap;
use std::convert::TryFrom;

#[cfg(feature = "big-table")]
use crate::tables::{MILLER_RABIN_BASE32, MILLER_RABIN_BASE64};

/// Fast primality test on a u64 integer. It's based on
/// deterministic Miller-rabin tests. if target is larger than 2^64 or more
/// controlled primality tests are desired, please use [is_prime()]
#[cfg(not(feature = "big-table"))]
pub fn is_prime64(target: u64) -> bool {
    // shortcuts
    if target < 1 {
        return false;
    }
    if target & 1 == 0 {
        return target == 2;
    }

    // first find in the prime list
    if let Ok(u) = u8::try_from(target) {
        return SMALL_PRIMES.binary_search(&u).is_ok();
    }

    // Then do a deterministic Miller-rabin test
    // The collection of witnesses are from http://miller-rabin.appspot.com/
    if let Ok(u) = u16::try_from(target) {
        // 2, 3 for u16 range
        return u.is_sprp(2) && u.is_sprp(3);
    }
    if let Ok(u) = u32::try_from(target) {
        // 2, 7, 61 for u32 range
        return u.is_sprp(2) && u.is_sprp(7) && u.is_sprp(61);
    }

    // 2, 325, 9375, 28178, 450775, 9780504, 1795265022 for u64 range
    const WITNESS64: [u64; 7] = [2, 325, 9375, 28178, 450775, 9780504, 1795265022];
    WITNESS64.iter().all(|&x| target.is_sprp(x))
}

/// Very fast primality test on a u64 integer is a prime number. It's based on
/// deterministic Miller-rabin tests with hashing. if target is larger than 2^64 or more controlled
/// primality tests are desired, please use is_prime() or PrimeBuffer::is_prime()
#[cfg(feature = "big-table")]
pub fn is_prime64(target: u64) -> bool {
    // shortcuts
    if target < 1 {
        return false;
    }
    if target & 1 == 0 {
        return target == 2;
    }

    // first find in the prime list
    if target < 8167 {
        return SMALL_PRIMES.binary_search(&(target as u16)).is_ok();
    }

    // 32bit test
    const MAGIC: u32 = 0xAD625B89;
    if let Ok(u) = u32::try_from(target) {
        let base = u.wrapping_mul(MAGIC) >> 24;
        return u.is_sprp(MILLER_RABIN_BASE32[base as usize]);
    }

    // 49bit test
    if !target.is_sprp(2) {
        return false;
    }
    let u = target as u32; // truncate
    let base = u.wrapping_mul(MAGIC) >> 18;
    if !target.is_sprp(MILLER_RABIN_BASE64[base as usize] as u64) {
        return false;
    }
    if target < (1u64 << 49) {
        return true;
    }

    // 64bit test
    const SECOND_BASES: [u64; 8] = [15, 135, 13, 60, 15, 117, 65, 29];
    let base = base >> 13;
    target.is_sprp(SECOND_BASES[base as usize])
}

/// Fast integer factorization on a u64 target. It's based on pollard's rho method and SQUFOF.
/// if target is larger than 2^64 or more controlled primality tests are desired, please use [is_prime()].
///
/// The factorization can be quite faster under 2^64 because: 1) faster and deterministic primality check,
/// 2) efficient montgomery multiplication implementation of u64
pub fn factors64(target: u64) -> BTreeMap<u64, usize> {
    // TODO: improve factorization performance
    // REF: http://flintlib.org/doc/ulong_extras.html#factorisation
    //      https://mathoverflow.net/questions/114018/fastest-way-to-factor-integers-260
    //      https://hal.inria.fr/inria-00188645v3/document
    //      https://github.com/coreutils/coreutils/blob/master/src/factor.c
    //      https://github.com/uutils/coreutils/blob/master/src/uu/factor/src/cli.rs
    //      https://github.com/elmomoilanen/prime-factorization
    //      https://github.com/radii/msieve
    //      Pari/GP: ifac_crack
    let mut result = BTreeMap::new();
    let f2 = target.trailing_zeros(); // quick check on factors of 2
    if f2 == 0 {
        if is_prime64(target) {
            result.insert(target, 1);
            return result;
        }
    } else {
        result.insert(2, f2 as usize);
    }

    // trial division using primes in the table
    let tsqrt = target.sqrt() + 1;

    let mut residual = target >> f2;
    let mut factored = false;

    #[cfg(not(feature = "big-table"))]
    for &p in SMALL_PRIMES.iter().skip(1) {
        let p64 = p as u64;
        if p64 > tsqrt {
            factored = true;
            break;
        }

        while residual % p64 == 0 {
            residual = residual / p64;
            *result.entry(p64).or_insert(0) += 1;
        }
        if residual == 1 {
            factored = true;
            break;
        }
    }

    #[cfg(feature = "big-table")]
    // divisibility check with pre-computed tables
    for (&p, (&pinv, plim)) in SMALL_PRIMES
        .iter()
        .zip(SMALL_PRIMES_INV.iter().zip(SMALL_PRIMES_INVLIM))
        .skip(1)
    {
        let p64 = p as u64;
        if p64 > tsqrt {
            factored = true;
            break;
        }

        let mut r = residual;
        let mut k: u32 = 0;
        loop {
            let r2 = r.wrapping_mul(pinv);

            if r2 <= plim {
                k += 1;
                r = r2;
            } else {
                break;
            }
        }
        if k > 0 {
            residual = residual / p64.pow(k);
            result.insert(p64, k as usize);
        }
        if residual == 1 {
            factored = true;
            break;
        }
    }

    if factored {
        if residual != 1 {
            result.insert(residual, 1);
        }
        return result;
    }

    // then try pollard's rho and SQUFOF methods util fully factored
    let mut todo = vec![residual];
    const SQUFOF_MULTIPLIERS: [u16; 16] = [
        1,
        3,
        5,
        7,
        11,
        3 * 5,
        3 * 7,
        3 * 11,
        5 * 7,
        5 * 11,
        7 * 11,
        3 * 5 * 7,
        3 * 5 * 11,
        3 * 7 * 11,
        5 * 7 * 11,
        3 * 5 * 7 * 11,
    ];
    while let Some(target) = todo.pop() {
        if is_prime64(target) {
            *result.entry(target).or_insert(0) += 1;
        } else {
            let mut i = 1usize;
            let divisor = loop {
                // try SQUFOF after 4 failed pollard rho trials
                if i % 5 == 0 && (i / 5) < SQUFOF_MULTIPLIERS.len() {
                    // TODO: check if the residual is a sqaure number before SQUFOF
                    if let Some(p) = squfof(&target, SQUFOF_MULTIPLIERS[i / 5] as u64) {
                        break p;
                    }
                } else {
                    let start = random::<u64>() % target;
                    let offset = random::<u64>() % target;
                    if let Some(p) = pollard_rho(&target, start, offset) {
                        break p;
                    }
                }
                i += 1;
            };
            todo.push(divisor);
            todo.push(target / divisor);
        }
    }
    result
}

/// This function re-exports [crate::buffer::PrimeBufferExt::is_prime()] with a default buffer distance
pub fn is_prime<T: PrimalityBase>(target: &T, config: Option<PrimalityTestConfig>) -> Primality
where
    for<'r> &'r T: PrimalityRefBase<T>,
{
    NaiveBuffer::new().is_prime(target, config)
}

/// This function re-exports [crate::buffer::PrimeBufferExt::factors()] with a default buffer instance
pub fn factors<T: PrimalityBase>(
    target: T,
    config: Option<FactorizationConfig>,
) -> Result<BTreeMap<T, usize>, Vec<T>>
where
    for<'r> &'r T: PrimalityRefBase<T>,
{
    NaiveBuffer::new().factors(target, config)
}

/// This function re-exports [NaiveBuffer::primes()] and collect result as a vector.
pub fn primes(limit: u64) -> Vec<u64> {
    NaiveBuffer::new().into_primes(limit).collect()
}

/// This function re-exports [NaiveBuffer::nprimes()] and collect result as a vector.
pub fn nprimes(count: usize) -> Vec<u64> {
    NaiveBuffer::new().into_nprimes(count).collect()
}

/// This function re-exports [NaiveBuffer::prime_pi()]
pub fn prime_pi(limit: u64) -> u64 {
    // TODO (v0.2): Implement stand alone prime_pi with Meissel-Lehmer method
    NaiveBuffer::new().prime_pi(limit)
}

/// This function re-exports [NaiveBuffer::nth_prime()]
pub fn nth_prime(n: u64) -> u64 {
    // TODO (v0.2): Implement stand alone nth_prime with prime_pi and next_prime/prev_prime
    NaiveBuffer::new().nth_prime(n)
}

/// This function re-exports [NaiveBuffer::primorial()]
pub fn primorial<T: PrimalityBase + std::iter::Product>(n: usize) -> T {
    NaiveBuffer::new()
        .into_nprimes(n)
        .map(|p| T::from_u64(p).unwrap())
        .product()
}

/// This function calculate the Möbius μ(n) function of the input integer
///
/// If the input integer is very hard to factorize, it's better to use
/// the [factors()] function to control how the factorization is done.
///
/// # Panics
/// if the factorization failed on target.
pub fn moebius_mu<T: PrimalityBase>(target: &T) -> i8
where
    for<'r> &'r T: PrimalityRefBase<T>,
{
    // remove factor 2
    if target.is_even() {
        let two = T::one() + T::one();
        let four = &two + &two;
        if (target % four).is_zero() {
            return 0;
        } else {
            return -moebius_mu(&(target / &two));
        }
    }

    // look up tables when input is smaller than 256
    if let Some(v) = (target - T::one()).to_u8() {
        let m = MOEBIUS_ODD[(v >> 6) as usize];
        let m = m & (3 << (v & 63));
        let m = m >> (v & 63);
        return m as i8 - 1;
    }

    // short cut for common primes
    let three_sq = T::from_u8(9).unwrap();
    let five_sq = T::from_u8(25).unwrap();
    let seven_sq = T::from_u8(49).unwrap();
    if (target % three_sq).is_zero()
        || (target % five_sq).is_zero()
        || (target % seven_sq).is_zero()
    {
        return 0;
    }

    // then try complete factorization
    let config = Some(FactorizationConfig::strict());
    match factors(target.clone(), config) {
        Ok(result) => {
            for exp in result.values() {
                if exp > &1 {
                    return 0;
                }
            }
            return if result.len() % 2 == 0 { 1 } else { -1 };
        }
        Err(_) => {
            panic!("Failed to factor the integer!");
        }
    }
}

/// Tests if the integer doesn't have any square number factor.
///
/// # Panics
/// if the factorization failed on target.
pub fn is_square_free<T: PrimalityBase>(target: &T) -> bool
where
    for<'r> &'r T: PrimalityRefBase<T>,
{
    moebius_mu(target) != 0
}

/// Returns the estimated bounds (low, high) of prime π function, such that
/// low <= π(target) <= high
///
/// # Reference:
/// - \[1] Dusart, Pierre. "Estimates of Some Functions Over Primes without R.H."
/// [arxiv:1002.0442](http://arxiv.org/abs/1002.0442). 2010.
/// - \[2] Dusart, Pierre. "Explicit estimates of some functions over primes."
/// The Ramanujan Journal 45.1 (2018): 227-251.
pub fn prime_pi_bounds<T: ToPrimitive + FromPrimitive>(target: &T) -> (T, T) {
    if let Some(x) = target.to_u64() {
        // use existing primes and return exact value
        if x <= (*SMALL_PRIMES.last().unwrap()) as u64 {
            #[cfg(not(feature = "big-table"))]
            let pos = SMALL_PRIMES.binary_search(&(x as u8));
            #[cfg(feature = "big-table")]
            let pos = SMALL_PRIMES.binary_search(&(x as u16));

            let n = match pos {
                Ok(p) => p + 1,
                Err(p) => p,
            };
            return (T::from_usize(n).unwrap(), T::from_usize(n).unwrap());
        }

        // use function approximation
        let n = x as f64;
        let ln = n.ln();
        let invln = ln.recip();

        let lo = match () {
            // [2] Collary 5.3
            _ if x >= 468049 => n / (ln - 1. - invln),
            // [2] Collary 5.2
            _ if x >= 88789 => n * invln * (1. + invln * (1. + 2. * invln)),
            // [2] Collary 5.3
            _ if x >= 5393 => n / (ln - 1.),
            // [2] Collary 5.2
            _ if x >= 599 => n * invln * (1. + invln),
            // [2] Collary 5.2
            _ => n * invln,
        };
        let hi = match () {
            // [2] Theorem 5.1, valid for x > 4e9, intersects at 7.3986e9
            _ if x >= 7398600000 => n * invln * (1. + invln * (1. + invln * (2. + invln * 7.59))),
            // [1] Theorem 6.9
            _ if x >= 2953652287 => n * invln * (1. + invln * (1. + invln * 2.334)),
            // [2] Collary 5.3, valid for x > 5.6, intersects at 5668
            _ if x >= 467345 => n / (ln - 1. - 1.2311 * invln),
            // [2] Collary 5.2, valid for x > 1, intersects at 29927
            _ if x >= 29927 => n * invln * (1. + invln * (1. + invln * 2.53816)),
            // [2] Collary 5.3, valid for x > exp(1.112), intersects at 5668
            _ if x >= 5668 => n / (ln - 1.112),
            // [2] Collary 5.2, valid for x > 1, intersects at 148
            _ if x >= 148 => n * invln * (1. + invln * 1.2762),
            // [2] Collary 5.2, valid for x > 1
            _ => 1.25506 * n * invln,
        };
        (T::from_f64(lo).unwrap(), T::from_f64(hi).unwrap())
    } else {
        let n = target.to_f64().unwrap();
        let ln = n.ln();
        let invln = ln.recip();

        // best bounds so far
        let lo = n / (ln - 1. - invln);
        let hi = n * invln * (1. + invln * (1. + invln * (2. + invln * 7.59)));
        (T::from_f64(lo).unwrap(), T::from_f64(hi).unwrap())
    }
}

/// Returns the estimated inclusive bounds (low, high) of the n-th prime. If the result
/// is larger than maximum of `T`, [None] will be returned.
///
/// # Reference:
/// - \[1] Dusart, Pierre. "Estimates of Some Functions Over Primes without R.H."
/// arXiv preprint [arXiv:1002.0442](https://arxiv.org/abs/1002.0442) (2010).
/// - \[2] Rosser, J. Barkley, and Lowell Schoenfeld. "Approximate formulas for some
/// functions of prime numbers." Illinois Journal of Mathematics 6.1 (1962): 64-94.
/// - \[3] Dusart, Pierre. "The k th prime is greater than k (ln k+ ln ln k-1) for k≥ 2."
/// Mathematics of computation (1999): 411-415.
/// - \[4] Axler, Christian. ["New Estimates for the nth Prime Number."](https://www.emis.de/journals/JIS/VOL22/Axler/axler17.pdf)
/// Journal of Integer Sequences 22.2 (2019): 3.
/// - \[5] Axler, Christian. [Uber die Primzahl-Zählfunktion, die n-te Primzahl und verallgemeinerte Ramanujan-Primzahlen. Diss.](http://docserv.uniduesseldorf.de/servlets/DerivateServlet/Derivate-28284/pdfa-1b.pdf)
/// PhD thesis, Düsseldorf, 2013.
///
/// Note that some of the results might depend on the Riemann Hypothesis. If you find
/// any prime that doesn't fall in the bound, then it might be a big discovery!
pub fn nth_prime_bounds<T: ToPrimitive + FromPrimitive>(target: &T) -> Option<(T, T)> {
    if let Some(x) = target.to_usize() {
        if x == 0 {
            return Some((T::from_u8(0).unwrap(), T::from_u8(0).unwrap()));
        }

        // use existing primes and return exact value
        if x <= SMALL_PRIMES.len() {
            let p = SMALL_PRIMES[x - 1];

            #[cfg(not(feature = "big-table"))]
            return Some((T::from_u8(p).unwrap(), T::from_u8(p).unwrap()));

            #[cfg(feature = "big-table")]
            return Some((T::from_u16(p).unwrap(), T::from_u16(p).unwrap()));
        }

        // use function approximation
        let n = x as f64;
        let ln = n.ln();
        let lnln = ln.ln();

        let lo = match () {
            // [4] Theroem 4, valid for x >= 2, intersects as 3.172e5
            _ if x >= 317200 => {
                n * (ln + lnln - 1. + (lnln - 2.) / ln
                    - (lnln * lnln - 6. * lnln + 11.321) / (2. * ln * ln))
            }
            // [1] Proposition 6.7, valid for x >= 3, intersects at 3520
            _ if x >= 3520 => n * (ln + lnln - 1. + (lnln - 2.1) / ln),
            // [3] title
            _ => n * (ln + lnln - 1.),
        };
        let hi = match () {
            // [4] Theroem 1, valid for x >= 46254381
            _ if x >= 46254381 => {
                n * (ln + lnln - 1. + (lnln - 2.) / ln
                    - (lnln * lnln - 6. * lnln + 10.667) / (2. * ln * ln))
            }
            // [5] Korollar 2.11, valid for x >= 8009824
            _ if x >= 8009824 => {
                n * (ln + lnln - 1. + (lnln - 2.) / ln
                    - (lnln * lnln - 6. * lnln + 10.273) / (2. * ln * ln))
            }
            // [1] Proposition 6.6
            _ if x >= 688383 => n * (ln + lnln - 1. + (lnln - 2.) / ln),
            // [1] Lemma 6.5
            _ if x >= 178974 => n * (ln + lnln - 1. + (lnln - 1.95) / ln),
            // [3] in "Further Results"
            _ if x >= 39017 => n * (ln + lnln - 0.9484),
            // [3] in "Further Results"
            _ if x >= 27076 => n * (ln + lnln - 1. + (lnln - 1.8) / ln),
            // [2] Theorem 3, valid for x >= 20
            _ => n * (ln + lnln - 0.5),
        };
        Some((T::from_f64(lo)?, T::from_f64(hi)?))
    } else {
        let n = target.to_f64().unwrap();
        let ln = n.ln();
        let lnln = ln.ln();

        // best bounds so far
        let lo = n
            * (ln + lnln - 1. + (lnln - 2.) / ln
                - (lnln * lnln - 6. * lnln + 11.321) / (2. * ln * ln));
        let hi = n
            * (ln + lnln - 1. + (lnln - 2.) / ln
                - (lnln * lnln - 6. * lnln + 10.667) / (2. * ln * ln));
        Some((T::from_f64(lo)?, T::from_f64(hi)?))
    }
}

/// Test if the target is a safe prime with Sophie German's definition. It will use the
/// strict primality test configuration.
pub fn is_safe_prime<T: PrimalityBase>(target: &T) -> Primality
where
    for<'r> &'r T: PrimalityRefBase<T>,
{
    let buf = NaiveBuffer::new();
    let config = Some(PrimalityTestConfig::strict());

    // test (n-1)/2 first since its smaller
    let sophie_p = buf.is_prime(&(target >> 1), config);
    if matches!(sophie_p, Primality::No) {
        return sophie_p;
    }

    // and then test target itself
    let target_p = buf.is_prime(target, config);
    target_p & sophie_p
}

/// Find the first prime number larger than `target`. If the result causes an overflow,
/// then [None] will be returned
#[cfg(feature = "big-table")]
pub fn next_prime<T: PrimalityBase + CheckedAdd>(
    target: &T,
    config: Option<PrimalityTestConfig>,
) -> Option<T>
where
    for<'r> &'r T: PrimalityRefBase<T>,
{
    // first search in small primes
    if target <= &T::from_u8(255).unwrap() // shortcut for u8
        || target < &T::from_u16(*SMALL_PRIMES.last().unwrap()).unwrap()
    {
        let next = match SMALL_PRIMES.binary_search(&target.to_u16().unwrap()) {
            Ok(pos) => SMALL_PRIMES[pos + 1],
            Err(pos) => SMALL_PRIMES[pos],
        };
        return T::from_u16(next);
    }

    // then moving along the wheel
    let mut i = (target % T::from_u8(WHEEL_SIZE).unwrap()).to_u8().unwrap();
    let mut t = target.clone();
    loop {
        let offset = WHEEL_NEXT[i as usize];
        t = t.checked_add(&T::from_u8(offset).unwrap())?;
        i = i.addm(offset, &WHEEL_SIZE);
        if is_prime(&t, config).probably() {
            break Some(t);
        }
    }
}

/// Find the first prime number smaller than `target`. If target is less than 3, then [None]
/// will be returned.
#[cfg(feature = "big-table")]
pub fn prev_prime<T: PrimalityBase>(target: &T, config: Option<PrimalityTestConfig>) -> Option<T>
where
    for<'r> &'r T: PrimalityRefBase<T>,
{
    if target <= &(T::one() + T::one()) {
        return None;
    }

    // first search in small primes
    if target <= &T::from_u8(255).unwrap() // shortcut for u8
        || target < &T::from_u16(*SMALL_PRIMES.last().unwrap()).unwrap()
    {
        let next = match SMALL_PRIMES.binary_search(&target.to_u16().unwrap()) {
            Ok(pos) => SMALL_PRIMES[pos - 1],
            Err(pos) => SMALL_PRIMES[pos - 1],
        };
        return Some(T::from_u16(next).unwrap());
    }

    // then moving along the wheel
    let mut i = (target % T::from_u8(WHEEL_SIZE).unwrap()).to_u8().unwrap();
    let mut t = target.clone();
    loop {
        let offset = WHEEL_PREV[i as usize];
        t = t - T::from_u8(offset).unwrap();
        i = i.subm(offset, &WHEEL_SIZE);
        if is_prime(&t, config).probably() {
            break Some(t);
        }
    }
}

macro_rules! impl_randprime_prim {
    ($($T:ty)*) => {$(
        impl<R: Rng> RandPrime<$T> for R {
            #[inline]
            fn gen_prime(&mut self, bit_size: usize, config: Option<PrimalityTestConfig>) -> $T {
                if bit_size > (<$T>::BITS as usize) {
                    panic!("The given bit size limit exceeded the capacity of the integer type!")
                }
                let t: $T = self.gen();
                let t = t >> (<$T>::BITS - bit_size as u32);
                if is_prime64(t as u64) {
                    t
                } else {
                    // deterministic primality test will be used for integers under u64
                    match next_prime(&t, None) {
                        Some(p) => p,
                        None => self.gen_prime(bit_size, config),
                    }
                }
            }

            #[inline]
            fn gen_safe_prime(&mut self, bit_size: usize) -> $T {
                // deterministic primality test will be used for integers under u64
                let p = self.gen_prime(bit_size, None);

                // test (p-1)/2
                if is_prime64((p >> 1) as u64) {
                    return p;
                }
                // test 2p+1
                if let Some(p2) = p.checked_mul(2).and_then(|v| v.checked_add(1)) {
                    if is_prime64(p2 as u64) {
                        return p2;
                    }
                }

                // try again if failed
                self.gen_safe_prime(bit_size)
            }
        }
    )*}
}
impl_randprime_prim!(u8 u16 u32 u64);

impl<R: Rng> RandPrime<u128> for R {
    #[inline]
    fn gen_prime(&mut self, bit_size: usize, config: Option<PrimalityTestConfig>) -> u128 {
        if bit_size > (u128::BITS as usize) {
            panic!("The given bit size limit exceeded the capacity of the integer type!")
        }
        let t: u128 = self.gen();
        let t = t >> (u128::BITS - bit_size as u32);
        if is_prime(&t, config).probably() {
            t
        } else {
            match next_prime(&t, config) {
                Some(p) => p,
                None => self.gen_prime(bit_size, config),
            }
        }
    }

    #[inline]
    fn gen_safe_prime(&mut self, bit_size: usize) -> u128 {
        let p = self.gen_prime(bit_size, None);
        let config = Some(PrimalityTestConfig::strict());
        if is_prime(&(p >> 1), config).probably() {
            return p;
        }
        if let Some(p2) = p.checked_mul(2).and_then(|v| v.checked_add(1)) {
            if is_prime(&p2, config).probably() {
                return p2;
            }
        }

        self.gen_safe_prime(bit_size)
    }
}

#[cfg(feature = "big-int")]
impl<R: Rng> RandPrime<BigUint> for R {
    #[inline]
    fn gen_prime(&mut self, bit_size: usize, config: Option<PrimalityTestConfig>) -> BigUint {
        let t = self.gen_biguint(bit_size as u64);
        if is_prime(&t, config).probably() {
            t
        } else {
            match next_prime(&t, config) {
                Some(p) => p,
                None => self.gen_prime(bit_size, config),
            }
        }
    }

    #[inline]
    fn gen_safe_prime(&mut self, bit_size: usize) -> BigUint {
        let p = self.gen_prime(bit_size, None);
        let config = Some(PrimalityTestConfig::strict());
        if is_prime(&(&p >> 1u8), config).probably() {
            return p;
        }
        let p2 = (p << 1u8) + 1u8;
        if is_prime(&p2, config).probably() {
            return p2;
        }

        self.gen_safe_prime(bit_size)
    }
}

// TODO: More functions
// REF: http://www.numbertheory.org/gnubc/bc_programs.html
// REF: https://github.com/TilmanNeumann/java-math-library
// - is_smooth: checks if the smoothness bound is at least b
// - euler_phi: Euler's totient function
// - jordan_tot: Jordan's totient function
// Others include Louiville function, Mangoldt function, Dedekind psi function, Dickman rho function, etc..

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{prelude::SliceRandom, random};
    use std::iter::FromIterator;

    #[test]
    fn is_prime64_test() {
        // test small primes
        for x in 2..100 {
            assert_eq!(SMALL_PRIMES.contains(&x), is_prime64(x as u64));
        }

        // some large primes
        assert!(is_prime64(6469693333));
        assert!(is_prime64(13756265695458089029));
        assert!(is_prime64(13496181268022124907));
        assert!(is_prime64(10953742525620032441));
        assert!(is_prime64(17908251027575790097));

        // primes from examples in Bradley Berg's hash method
        assert!(is_prime64(480194653));
        assert!(!is_prime64(20074069));
        assert!(is_prime64(8718775377449));
        assert!(is_prime64(3315293452192821991));
        assert!(!is_prime64(8651776913431));
        assert!(!is_prime64(1152965996591997761));

        // ensure no factor for 100 random primes
        let mut rng = rand::thread_rng();
        for _ in 0..100 {
            let x = random();
            if !is_prime64(x) {
                continue;
            }
            assert_ne!(x % (*SMALL_PRIMES.choose(&mut rng).unwrap() as u64), 0);
        }

        // create random composites
        for _ in 0..100 {
            let x = random::<u32>() as u64;
            let y = random::<u32>() as u64;
            assert!(!is_prime64(x * y));
        }
    }

    #[test]
    fn factors64_test() {
        // some known cases
        let fac123456789 = BTreeMap::from_iter([(3, 2), (3803, 1), (3607, 1)]);
        let fac = factors64(123456789);
        assert_eq!(fac, fac123456789);

        let fac1_17 = BTreeMap::from_iter([(2071723, 1), (5363222357, 1)]);
        let fac = factors64(11111111111111111);
        assert_eq!(fac, fac1_17);

        // 100 random factorization tests
        for _ in 0..100 {
            let x = random();
            let fac = factors64(x);
            let mut prod = 1;
            for (p, exp) in fac {
                assert!(
                    is_prime64(p),
                    "factorization result should have prime factors! (get {})",
                    p
                );
                prod *= p.pow(exp as u32);
            }
            assert_eq!(x, prod, "factorization check failed! ({} != {})", x, prod);
        }
    }

    #[test]
    fn is_safe_prime_test() {
        // OEIS:A005385
        let safe_primes = [
            5, 7, 11, 23, 47, 59, 83, 107, 167, 179, 227, 263, 347, 359, 383, 467, 479, 503, 563,
            587, 719, 839, 863, 887, 983, 1019, 1187, 1283, 1307, 1319, 1367, 1439, 1487, 1523,
            1619, 1823, 1907,
        ];
        for p in SMALL_PRIMES {
            if p > 1500 {
                break;
            }
            assert_eq!(
                is_safe_prime(&p).probably(),
                safe_primes.iter().find(|&v| &p == v).is_some()
            );
        }
    }

    #[test]
    fn moebius_mu_test() {
        // test small examples
        let mu20: [i8; 20] = [
            1, -1, -1, 0, -1, 1, -1, 0, 0, 1, -1, 0, -1, 1, 1, 0, -1, 0, -1, 0,
        ];
        for i in 0..20 {
            assert_eq!(moebius_mu(&(i + 1)), mu20[i], "moebius on {}", i);
        }

        // some square numbers
        assert_eq!(moebius_mu(&1024u32), 0);
        assert_eq!(moebius_mu(&(8081u32 * 8081)), 0);

        // sphenic numbers
        let sphenic3: [u8; 20] = [
            30, 42, 66, 70, 78, 102, 105, 110, 114, 130, 138, 154, 165, 170, 174, 182, 186, 190,
            195, 222,
        ]; // OEIS:A007304
        for i in 0..20 {
            assert_eq!(moebius_mu(&sphenic3[i]), -1i8, "moebius on {}", sphenic3[i]);
        }
        let sphenic5: [u16; 23] = [
            2310, 2730, 3570, 3990, 4290, 4830, 5610, 6006, 6090, 6270, 6510, 6630, 7410, 7590,
            7770, 7854, 8610, 8778, 8970, 9030, 9282, 9570, 9690,
        ]; // OEIS:A046387
        for i in 0..20 {
            assert_eq!(moebius_mu(&sphenic5[i]), -1i8, "moebius on {}", sphenic5[i]);
        }
    }

    #[test]
    fn prime_pi_bounds_test() {
        fn check(n: u64, pi: u64) {
            let (lo, hi) = prime_pi_bounds(&n);
            assert!(
                lo <= pi && pi <= hi,
                "fail to satisfy {} <= pi({}) = {} <= {}",
                lo,
                n,
                pi,
                hi
            )
        }

        // test with sieved primes
        let mut pb = NaiveBuffer::new();
        let mut last = 0;
        for (i, p) in pb.primes(100000).cloned().enumerate() {
            println!("{}, {}", i, p);
            for j in last..p {
                check(j, i as u64);
            }
            last = p;
        }

        // test with some known cases with input as 10^n, OEIS:A006880
        let pow10_values = [
            0,
            4,
            25,
            168,
            1229,
            9592,
            78498,
            664579,
            5761455,
            50847534,
            455052511,
            4118054813,
            37607912018,
            346065536839,
            3204941750802,
            29844570422669,
            279238341033925,
            2623557157654233,
        ];
        for (exponent, gt) in pow10_values.iter().enumerate() {
            let n = 10u64.pow(exponent as u32);
            check(n, *gt);
        }
    }

    #[test]
    fn nth_prime_bounds_test() {
        fn check(n: u64, p: u64) {
            let (lo, hi) = super::nth_prime_bounds(&n).unwrap();
            assert!(
                lo <= p && p <= hi,
                "fail to satisfy: {} <= {}-th prime = {} <= {}",
                lo,
                n,
                p,
                hi
            );
        }

        // test with sieved primes
        let mut pb = NaiveBuffer::new();
        for (i, p) in pb.primes(100000).cloned().enumerate() {
            check(i as u64 + 1, p as u64);
        }

        // test with some known cases with input as 10^n, OEIS:A006988
        let pow10_values = [
            2,
            29,
            541,
            7919,
            104729,
            1299709,
            15485863,
            179424673,
            2038074743,
            22801763489,
            252097800623,
            2760727302517,
            29996224275833,
            323780508946331,
            3475385758524527,
            37124508045065437,
        ];
        for (exponent, nth_prime) in pow10_values.iter().enumerate() {
            let n = 10u64.pow(exponent as u32);
            check(n, *nth_prime);
        }
    }

    #[test]
    fn prev_next_test() {
        assert_eq!(prev_prime(&2u32, None), None);

        // OEIS:A077800
        let twine_primes: [(u32, u32); 8] = [
            (2, 3), // not exactly twine
            (3, 5),
            (5, 7),
            (11, 13),
            (17, 19),
            (29, 31),
            (41, 43),
            (617, 619),
        ];
        for (p1, p2) in twine_primes {
            assert_eq!(prev_prime(&p2, None).unwrap(), p1);
            assert_eq!(next_prime(&p1, None).unwrap(), p2);
        }

        let adj10_primes: [(u32, u32); 7] = [
            (7, 11),
            (97, 101),
            (997, 1009),
            (9973, 10007),
            (99991, 100003),
            (999983, 1000003),
            (9999991, 10000019),
        ];
        for (i, (p1, p2)) in adj10_primes.iter().enumerate() {
            assert_eq!(prev_prime(p2, None).unwrap(), *p1);
            assert_eq!(next_prime(p1, None).unwrap(), *p2);

            let pow = 10u32.pow((i + 1) as u32);
            assert_eq!(prev_prime(&pow, None).unwrap(), *p1);
            assert_eq!(next_prime(&pow, None).unwrap(), *p2);
        }
    }

    #[test]
    fn rand_prime_test() {
        let mut rng = rand::thread_rng();

        // test random prime generation for each size
        let p: u8 = rng.gen_prime(8, None);
        assert!(is_prime64(p as u64));
        let p: u16 = rng.gen_prime(16, None);
        assert!(is_prime64(p as u64));
        let p: u32 = rng.gen_prime(32, None);
        assert!(is_prime64(p as u64));
        let p: u64 = rng.gen_prime(64, None);
        assert!(is_prime64(p));
        let p: u128 = rng.gen_prime(128, None);
        assert!(is_prime(&p, None).probably());

        // test random safe prime generation
        let p: u8 = rng.gen_safe_prime(8);
        assert!(is_safe_prime(&p).probably());
        let p: u32 = rng.gen_safe_prime(32);
        assert!(is_safe_prime(&p).probably());
        let p: u128 = rng.gen_safe_prime(128);
        assert!(is_safe_prime(&p).probably());

        #[cfg(feature = "big-int")]
        {
            let p: BigUint = rng.gen_prime(512, None);
            assert!(is_prime(&p, None).probably());
            let p: BigUint = rng.gen_safe_prime(192);
            assert!(is_prime(&p, None).probably());
        }

        // test bit size limit
        let p: u16 = rng.gen_prime(12, None);
        assert!(p < (1 << 12));
        let p: u32 = rng.gen_prime(24, None);
        assert!(p < (1 << 24));
    }
}
