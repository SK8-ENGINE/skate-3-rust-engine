//! The reward summary and milestone card natives (FERewardSummary, research b873 §4, b899 §1.1 / 1.2).
//! Details: docs/hails-additions/43-career.md#change-the-reward-presentation-careerpresentationrs
use crate::apt_vm::{Value, Vm};
use skate_core::career::presentation::{Card, SummaryView};

#[derive(Clone, Default)]
pub struct RewardSummaryHost {
    pub view: SummaryView,
    /// The countdown the summary shows ("Going to Rewards in {0}").
    pub timer: f64,
}

impl RewardSummaryHost {
    /// A FERewardSummary member call; None for anything else.
    pub fn call(&self, vm: &mut Vm, object: &str, method: &str, args: &[Value]) -> Result<Option<Value>, String> {
        if object != "FERewardSummary" {
            return Ok(None);
        }
        let v = &self.view;
        let n = |x: u64| Value::Number(x as f64);
        Ok(Some(match method {
            "GetTimerValue" => Value::Number(self.timer),
            "GetNextBoardSaleMilestone" => Value::Number(v.next_milestone),
            "GetPrevBoardSaleMilestone" => Value::Number(v.prev_milestone),
            "GetBaseCashReward" => n(v.totals.cash_owned),
            "GetKilledBonusCashReward" => n(v.totals.cash_killed),
            "GetOnlineBoardSalesReward" => n(0),
            "GetOwnedBoardSalesReward" => n(v.totals.sales_owned),
            "GetKilledBoardSalesReward" => n(v.totals.sales_killed),
            "GetTotalCash" => n(v.total_cash),
            "GetTotalBoardSales" => n(v.total_sales),
            "GetUnlockCategoryNames" => {
                let titles = v.tabs.iter().map(|(t, _)| Value::Text(t.title.clone())).collect();
                Value::Object(vm.array(titles)?)
            }
            "GetUnlockCategoryIDs" => {
                let ids = v.tabs.iter().map(|(t, _)| Value::Text(t.id.clone())).collect();
                Value::Object(vm.array(ids)?)
            }
            // UNLOCKDATA_DESC 0 / ICON 1 / ONLINE 2.
            "GetUnlocksInCategory" => match self.tab(args) {
                Some((_, rows)) => {
                    let online = rows.iter().map(|_| Value::Bool(false)).collect();
                    columns(vm, rows, |c| &c.title, |c| &c.image, Some(online))?
                }
                None => columns(vm, &[], |c| &c.title, |c| &c.image, Some(Vec::new()))?,
            },
            "IsCategoryShownAsList" => Value::Bool(self.tab(args).is_some_and(|(t, _)| t.list)),
            // MILESTONE_UNLOCK_NAME 0 / ICON_PATH 1 / DESCRIPTION 2.
            "GetMilestoneUnlockInfo" => {
                let desc = v.milestones.iter().map(|c| Value::Text(c.description.clone())).collect();
                columns(vm, &v.milestones, |c| &c.title, |c| &c.image, Some(desc))?
            }
            _ => return Ok(None),
        }))
    }

    /// The tab a call names: its index or its id.
    fn tab(&self, args: &[Value]) -> Option<&(skate_core::career::presentation::Tab, Vec<Card>)> {
        match args.first()? {
            Value::Number(i) if *i >= 0.0 => self.view.tabs.get(*i as usize),
            Value::Text(id) => self.view.tabs.iter().find(|(t, _)| &t.id == id),
            _ => None,
        }
    }
}

/// Three parallel arrays: first, second, and the third column.
fn columns(vm: &mut Vm, cards: &[Card], a: fn(&Card) -> &String, b: fn(&Card) -> &String, third: Option<Vec<Value>>)
    -> Result<Value, String> {
    let first = cards.iter().map(|c| Value::Text(a(c).clone())).collect();
    let second = cards.iter().map(|c| Value::Text(b(c).clone())).collect();
    let mut out = vec![Value::Object(vm.array(first)?), Value::Object(vm.array(second)?)];
    if let Some(third) = third {
        out.push(Value::Object(vm.array(third)?));
    }
    Ok(Value::Object(vm.array(out)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_core::career::presentation::Tab;

    fn host() -> RewardSummaryHost {
        let card = |t: &str| Card { title: t.into(), image: format!("{t}.img"), description: format!("{t}.desc") };
        let tab = Tab { id: "gear".into(), title: "ID_REWARD_TAB_GEAR".into(), list: true, kinds: vec![5] };
        let mut view = SummaryView { tabs: vec![(tab, vec![card("shoe")])], milestones: vec![card("m1"), card("m2")], ..Default::default() };
        view.totals.cash_owned = 500;
        RewardSummaryHost { view, timer: 3.0 }
    }

    fn item(vm: &mut Vm, v: &Value, i: &str) -> Value {
        match v {
            Value::Object(id) => vm.get(*id, i),
            _ => panic!("not an array"),
        }
    }

    #[test]
    fn summary_natives_answer_from_the_view() {
        let h = host();
        let mut vm = Vm::new();
        assert_eq!(h.call(&mut vm, "FERewardSummary", "GetBaseCashReward", &[]).unwrap(), Some(Value::Number(500.0)));
        let names = h.call(&mut vm, "FERewardSummary", "GetUnlockCategoryNames", &[]).unwrap().unwrap();
        assert_eq!(item(&mut vm, &names, "0"), Value::Text("ID_REWARD_TAB_GEAR".into()));
        let rows = h.call(&mut vm, "FERewardSummary", "GetUnlocksInCategory", &[Value::Number(0.0)]).unwrap().unwrap();
        let icons = item(&mut vm, &rows, "1");
        assert_eq!(item(&mut vm, &icons, "0"), Value::Text("shoe.img".into()));
        assert_eq!(h.call(&mut vm, "FERewardSummary", "IsCategoryShownAsList", &[Value::Text("gear".into())]).unwrap(),
            Some(Value::Bool(true)));
        assert_eq!(h.call(&mut vm, "Progression", "GetTotalCash", &[]).unwrap(), None);
    }

    #[test]
    fn milestone_info_is_three_parallel_arrays() {
        let h = host();
        let mut vm = Vm::new();
        let info = h.call(&mut vm, "FERewardSummary", "GetMilestoneUnlockInfo", &[]).unwrap().unwrap();
        let desc = item(&mut vm, &info, "2");
        assert_eq!(item(&mut vm, &desc, "1"), Value::Text("m2.desc".into()));
    }
}
