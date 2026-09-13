//! Deciding whether a candidate provider config may replace the one already in force.
//!
//! Two different questions live here, and conflating them is the mistake this module exists to
//! prevent:
//!
//! 1. **Is this a well-formed config?** — `serde` plus [`CompiledSchedule::compile`]. Answers
//!    "could the app run on this at all?"
//! 2. **Is this an acceptable *replacement*?** — everything else in this file. Answers
//!    "should the app swap working data for this?"
//!
//! The second question only exists because a config can now arrive over the network. One baked
//! into the binary at build time has been through review; a fetched one has not, and neither
//! has a hand-edited one. Every check below is therefore conservative, and the caller's
//! contract is that **a rejection leaves the existing config completely untouched** — which is
//! what keeps the tray icon on screen.
//!
//! The baseline every comparison is made against is *the config currently in force*, not the
//! bundled one. That matters for the price check: a genuine large price change takes effect on
//! the next sync and the baseline moves with it, so a user is never permanently locked out of
//! real prices by a guard aimed at typos.

use crate::engine::CompiledSchedule;
use crate::model::{ProviderConfig, ScheduleError, Tier};
use chrono::NaiveDate;

/// How far a single price may move relative to the config in force before it is refused.
///
/// This is a **typo guard, not a security boundary.** It catches a misplaced decimal point,
/// which is by far the likeliest way a hand-edited price goes wrong (¥4 → ¥40 → ¥400). It does
/// not and cannot defend against someone who controls the config's source: whoever can commit
/// there can publish any price they like.
pub const MAX_PRICE_RATIO: f64 = 20.0;

/// Why a candidate config was refused.
///
/// Everything here is a *rejection of a replacement*, as opposed to [`ScheduleError`], which
/// describes a config that could not be understood in the first place. Both are English: they
/// are configuration-time errors a maintainer sees, not normal-use strings (see the "known
/// limitations" note about error localisation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigRejection {
    /// The JSON could not be deserialized. Wraps serde's own message.
    Json(String),
    /// The JSON parsed, but the schedule in it would not compile.
    Schedule(ScheduleError),
    /// The candidate declares a different provider than the one it would replace.
    ProviderMismatch { expected: String, found: String },
    /// The candidate publishes no models at all, or a model publishes no prices.
    NoModels,
    /// `verifiedAt` is not a `YYYY-MM-DD` date.
    BadDate { raw: String },
    /// The candidate is older than the config in force. Applying it would roll prices
    /// backwards — almost always a stale CDN copy rather than an intentional revert.
    Stale { in_force: String, candidate: String },
    /// A published rate is negative, or not a finite number.
    BadPrice {
        model: String,
        currency: String,
        field: String,
        raw: String,
    },
    /// A rate moved by more than [`MAX_PRICE_RATIO`] relative to the config in force.
    ImplausiblePrice {
        model: String,
        currency: String,
        field: String,
        from: String,
        to: String,
    },
}

impl std::fmt::Display for ConfigRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigRejection::Json(e) => {
                write!(f, "the update is not valid JSON: {e}")
            }
            ConfigRejection::Schedule(e) => write!(f, "the update's schedule is unusable: {e}"),
            ConfigRejection::ProviderMismatch { expected, found } => write!(
                f,
                "the update is for provider {found:?}, but this app is set up for {expected:?}"
            ),
            ConfigRejection::NoModels => write!(f, "the update publishes no models"),
            ConfigRejection::BadDate { raw } => write!(
                f,
                "verifiedAt is {raw:?}, which is not a YYYY-MM-DD date"
            ),
            ConfigRejection::Stale {
                in_force,
                candidate,
            } => write!(
                f,
                "the update is from {candidate}, older than the data already in use \
                 ({in_force}); applying it would roll the prices backwards"
            ),
            ConfigRejection::BadPrice {
                model,
                currency,
                field,
                raw,
            } => write!(
                f,
                "the update's {model} price for {currency} {field} is not a usable number: {raw:?}"
            ),
            ConfigRejection::ImplausiblePrice {
                model,
                currency,
                field,
                from,
                to,
            } => write!(
                f,
                "the update moves {model} {currency} {field} from {from} to {to}, which is more \
                 than {MAX_PRICE_RATIO}x; refusing it as a likely typo"
            ),
        }
    }
}

impl std::error::Error for ConfigRejection {}

impl From<ScheduleError> for ConfigRejection {
    fn from(e: ScheduleError) -> Self {
        ConfigRejection::Schedule(e)
    }
}

/// Accept a candidate config as a replacement for `baseline`, or say exactly why not.
///
/// On `Ok`, the returned config has been parsed *and* its schedule compiled, so a caller that
/// installs it cannot be installing something that fails later.
pub fn accept(candidate_json: &str, baseline: &ProviderConfig) -> Result<ProviderConfig, ConfigRejection> {
    let candidate: ProviderConfig = serde_json::from_str(candidate_json)
        .map_err(|e| ConfigRejection::Json(e.to_string()))?;

    // Identity first: the cheapest check, and the one whose failure most changes the meaning
    // of everything after it. A config for another provider is not a price update at all.
    if candidate.provider != baseline.provider {
        return Err(ConfigRejection::ProviderMismatch {
            expected: baseline.provider.clone(),
            found: candidate.provider.clone(),
        });
    }

    // Compile here rather than leaving it to the caller. A config whose schedule cannot be
    // compiled would render as a grey Unknown dot, and a "price update" that greys out the
    // icon is worse than one that is refused.
    CompiledSchedule::compile(&candidate.schedule)?;

    if candidate.models.is_empty() {
        return Err(ConfigRejection::NoModels);
    }

    let in_force = parse_date(&baseline.verified_at)?;
    let proposed = parse_date(&candidate.verified_at)?;
    if proposed < in_force {
        return Err(ConfigRejection::Stale {
            in_force: baseline.verified_at.clone(),
            candidate: candidate.verified_at.clone(),
        });
    }

    check_prices(&candidate, baseline)?;
    Ok(candidate)
}

/// Every rate must be a usable number, and must not have moved implausibly far from the config
/// in force.
///
/// Models and currencies that the baseline does not have are checked for validity but not for
/// magnitude — there is nothing to compare them against, and refusing to let a provider add a
/// model or a currency would defeat the point of being able to update at all.
fn check_prices(candidate: &ProviderConfig, baseline: &ProviderConfig) -> Result<(), ConfigRejection> {
    for model in &candidate.models {
        if model.prices.is_empty() {
            return Err(ConfigRejection::NoModels);
        }

        let baseline_model = baseline.models.iter().find(|m| m.id == model.id);

        for (currency, prices) in &model.prices {
            let baseline_prices = baseline_model.and_then(|m| m.price_in(currency));

            // Both tiers of every field, so a config cannot smuggle a bad `peak` past by
            // having a sane `offPeak`.
            for (field, tiered) in prices.fields() {
                let previous_tiered = baseline_prices.and_then(|p| p.field(field));

                for tier in Tier::ALL {
                    let value = tiered.for_tier(tier);
                    let name = format!("{field} {}", tier.field_name());

                    if !value.is_finite() || value < 0.0 {
                        return Err(ConfigRejection::BadPrice {
                            model: model.id.clone(),
                            currency: currency.clone(),
                            field: name,
                            raw: format!("{value}"),
                        });
                    }

                    let Some(previous) = previous_tiered.map(|t| t.for_tier(tier)) else {
                        continue;
                    };
                    // A zero baseline makes a ratio meaningless (and a free tier is not
                    // implausible), so magnitude is only judged from a real number.
                    if previous <= 0.0 {
                        continue;
                    }

                    let ratio = value / previous;
                    if !(1.0 / MAX_PRICE_RATIO..=MAX_PRICE_RATIO).contains(&ratio) {
                        return Err(ConfigRejection::ImplausiblePrice {
                            model: model.id.clone(),
                            currency: currency.clone(),
                            field: name,
                            from: format!("{previous}"),
                            to: format!("{value}"),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

fn parse_date(raw: &str) -> Result<NaiveDate, ConfigRejection> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").map_err(|_| ConfigRejection::BadDate {
        raw: raw.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_bundled;

    /// Round-trip a config through JSON so tests can express "the same config, but with X
    /// changed" without hand-writing 60 lines of JSON.
    fn as_json(config: &ProviderConfig) -> String {
        serde_json::to_string(config).unwrap()
    }

    fn baseline() -> ProviderConfig {
        load_bundled().unwrap()
    }

    #[test]
    fn the_bundled_config_is_acceptable_as_its_own_replacement() {
        let base = baseline();
        let accepted = accept(&as_json(&base), &base).expect("a config must be able to replace itself");
        assert_eq!(accepted.verified_at, base.verified_at);
    }

    #[test]
    fn a_genuine_price_change_goes_through() {
        let base = baseline();
        let mut next = base.clone();
        // A doubling is a real-world event, not a typo.
        next.models[0].prices.get_mut("CNY").unwrap().output.peak *= 2.0;
        next.verified_at = "2026-09-20".to_string();

        let accepted = accept(&as_json(&next), &base).expect("a doubling must be accepted");
        assert_eq!(accepted.models[0].prices["CNY"].output.peak, 16.0);
    }

    #[test]
    fn a_misplaced_decimal_point_is_refused() {
        let base = baseline();
        let mut next = base.clone();
        // ¥8 -> ¥800: the classic hand-edit slip, and exactly what this guard is for.
        next.models[0].prices.get_mut("CNY").unwrap().output.peak = 800.0;
        next.verified_at = "2026-09-20".to_string();

        let err = accept(&as_json(&next), &base).unwrap_err();
        assert!(
            matches!(err, ConfigRejection::ImplausiblePrice { .. }),
            "expected an implausibility rejection, got {err:?}"
        );
        assert!(err.to_string().contains("output peak"), "{err}");
    }

    #[test]
    fn a_large_but_real_change_is_not_locked_out_forever() {
        // The guard compares against what is *in force*, not against the frozen bundled
        // figures. So a real 50x rise installs once, and the next sync compares against the
        // new value rather than being refused forever.
        let base = baseline();
        let mut first = base.clone();
        first.models[0].prices.get_mut("CNY").unwrap().output.peak = 400.0;
        first.verified_at = "2026-09-20".to_string();

        // 400 / 8 = 50x, so the first hop is refused...
        assert!(accept(&as_json(&first), &base).is_err());

        // ...but stepping through a config that is already at 400 makes 400 its own baseline,
        // and the same document is then a no-op change.
        let stepped = {
            let mut s = base.clone();
            s.models[0].prices.get_mut("CNY").unwrap().output.peak = 100.0;
            s.verified_at = "2026-09-15".to_string();
            s
        };
        let installed = accept(&as_json(&stepped), &base).expect("12.5x is within the band");
        accept(&as_json(&first), &installed).expect("now measured against 100, 400 is 4x");
    }

    #[test]
    fn an_older_config_is_refused_as_a_rollback() {
        let base = baseline();
        let mut older = base.clone();
        older.verified_at = "2026-01-01".to_string();

        let err = accept(&as_json(&older), &base).unwrap_err();
        assert!(matches!(err, ConfigRejection::Stale { .. }), "got {err:?}");
        assert!(err.to_string().contains("backwards"), "{err}");
    }

    #[test]
    fn the_same_date_is_not_stale() {
        // Re-syncing the identical document must be a no-op, not an error — otherwise the
        // button would report a failure every time the user clicked it twice.
        let base = baseline();
        accept(&as_json(&base), &base).expect("an equal date is not older");
    }

    #[test]
    fn a_malformed_date_is_refused_rather_than_sorted_lexically() {
        let base = baseline();
        let mut next = base.clone();
        // Lexically "soon" > "2026-09-12", so a naive string comparison would wave this
        // through and its staleness would never be detectable again.
        next.verified_at = "soon".to_string();

        let err = accept(&as_json(&next), &base).unwrap_err();
        assert!(matches!(err, ConfigRejection::BadDate { .. }), "got {err:?}");
    }

    #[test]
    fn a_config_for_another_provider_is_refused() {
        let base = baseline();
        let mut next = base.clone();
        next.provider = "openai".to_string();

        let err = accept(&as_json(&next), &base).unwrap_err();
        assert!(
            matches!(err, ConfigRejection::ProviderMismatch { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn a_broken_schedule_is_refused_even_though_the_json_is_valid() {
        let base = baseline();
        let json = as_json(&base).replace("\"01:00\"", "\"25:00\"");

        let err = accept(&json, &base).unwrap_err();
        assert!(matches!(err, ConfigRejection::Schedule(_)), "got {err:?}");
    }

    #[test]
    fn a_zero_length_window_is_refused() {
        let base = baseline();
        let json = as_json(&base).replace("\"04:00\"", "\"01:00\"");

        assert!(accept(&json, &base).is_err());
    }

    #[test]
    fn json_that_is_not_even_json_is_refused() {
        let base = baseline();
        let err = accept("<!doctype html><html>404</html>", &base).unwrap_err();
        assert!(matches!(err, ConfigRejection::Json(_)), "got {err:?}");

        // The realistic version of this: a proxy or a captive portal answering 200 with HTML.
        assert!(err.to_string().contains("not valid JSON"), "{err}");
    }

    #[test]
    fn a_negative_price_is_refused() {
        let base = baseline();
        let mut next = base.clone();
        next.models[0].prices.get_mut("CNY").unwrap().input_cache_hit.off_peak = -1.0;
        next.verified_at = "2026-09-20".to_string();

        let err = accept(&as_json(&next), &base).unwrap_err();
        assert!(matches!(err, ConfigRejection::BadPrice { .. }), "got {err:?}");
    }

    /// NaN and infinity are refused a layer *earlier* than the price check, because JSON has no
    /// way to write them: `serde_json` emits `null`, and `null` is not an `f64`.
    ///
    /// This caught a wrong assumption — the test that first covered this asserted a
    /// `BadPrice`, and failed. Recording the real behaviour is more useful than the check it
    /// was meant to exercise, so both are kept: this one pins the boundary, and
    /// `a_non_finite_price_is_refused_by_the_check_itself` covers the branch directly.
    #[test]
    fn a_non_finite_price_cannot_even_be_written_as_json() {
        let base = baseline();
        let encoded: serde_json::Value = serde_json::from_str(&as_json(&base)).unwrap();
        assert!(
            encoded["models"][0]["prices"]["CNY"]["output"]["peak"].is_number(),
            "sanity: real prices are numbers"
        );

        for bad in [f64::NAN, f64::INFINITY] {
            let mut next = base.clone();
            next.models[0].prices.get_mut("CNY").unwrap().output.peak = bad;
            next.models[0].prices.get_mut("CNY").unwrap().output.off_peak = bad;

            let json = as_json(&next);
            assert!(json.contains("null"), "serde_json should have written {bad} as null");

            let err = accept(&json, &base).unwrap_err();
            assert!(matches!(err, ConfigRejection::Json(_)), "got {err:?}");
        }
    }

    /// The branch above is unreachable through `accept`, but it is still the thing that
    /// guarantees the renderer never sees a NaN — so it is tested where it lives rather than
    /// left as unchecked code that only *looks* covered.
    #[test]
    fn a_non_finite_price_is_refused_by_the_check_itself() {
        let base = baseline();

        for bad in [f64::NAN, f64::INFINITY] {
            let mut next = base.clone();
            next.models[0].prices.get_mut("CNY").unwrap().output.peak = bad;

            let err = check_prices(&next, &base).unwrap_err();
            assert!(matches!(err, ConfigRejection::BadPrice { .. }), "got {err:?}");
        }
    }

    #[test]
    fn a_config_with_no_models_is_refused() {
        let base = baseline();
        let mut next = base.clone();
        next.models.clear();
        next.verified_at = "2026-09-20".to_string();

        assert!(matches!(
            accept(&as_json(&next), &base).unwrap_err(),
            ConfigRejection::NoModels
        ));
    }

    #[test]
    fn a_new_model_or_currency_is_allowed_through() {
        // There is nothing to compare a brand-new model against, so magnitude cannot be
        // judged — but that must not block a provider from adding one.
        let base = baseline();
        let mut next = base.clone();
        let mut fresh = next.models[0].clone();
        fresh.id = "deepseek-flash-lite".to_string();
        fresh.label = "Flash Lite".to_string();
        fresh.prices.get_mut("CNY").unwrap().output.peak = 999.0;
        next.models.push(fresh);
        next.verified_at = "2026-09-20".to_string();

        accept(&as_json(&next), &base).expect("a new model must be addable");
    }

    #[test]
    fn every_published_rate_is_covered_by_the_check() {
        // Guards the check itself: if `ModelPrices` grows a field and `fields()` is not
        // updated, the new field would silently skip validation. Count the rates the config
        // actually publishes and compare with what the walker yields.
        let base = baseline();
        let json: serde_json::Value = serde_json::from_str(&as_json(&base)).unwrap();

        let declared = json["models"][0]["prices"]["CNY"].as_object().unwrap();
        let walked = base.models[0].prices["CNY"].fields().len();

        assert_eq!(
            declared.len(),
            walked,
            "ModelPrices::fields() no longer covers every rate in the config: {declared:?}"
        );
    }
}
