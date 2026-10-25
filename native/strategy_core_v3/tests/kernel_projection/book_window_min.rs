use super::*;
use strategy_core_kernel::{BookLevel, BookWindowMin};
use strategy_core_v3::decision_v4::{BookLevelV4, BookV4};
use strategy_core_v3::decision_v6::{
    BookWindowMinV6, DecisionV6Error, MAX_BOOK_WINDOW_MS, MarketBookWindowMinV6,
    decision_fence_v6_sha256,
};

fn level(price_micros: u64, quantity_hundredths: u64) -> BookLevelV4 {
    BookLevelV4 {
        price_micros,
        quantity_hundredths,
    }
}

fn with_book() -> DecisionContextV6 {
    let mut context = base_context();
    context.owner_state.markets[0].book = Some(BookV4 {
        snapshot_at_unix_ms: Some(1_000),
        yes_bids: vec![level(300_000, 500)],
        no_bids: vec![level(600_000, 2_500), level(550_000, 4_000)],
        ..BookV4::default()
    });
    context
}

fn window(yes: Vec<BookLevelV4>, no: Vec<BookLevelV4>) -> Option<BookWindowMinV6> {
    Some(BookWindowMinV6 {
        window_ms: 2_000,
        markets: vec![MarketBookWindowMinV6 {
            market_id: MARKET.to_owned(),
            yes_bids: yes,
            no_bids: no,
        }],
    })
}

#[test]
fn window_minimums_reach_the_kernel_level_for_level_and_absent_stays_absent() {
    let context = with_book();
    let decoded =
        decode_decision_context_v6(&encode_decision_context_v6(&context).unwrap()).unwrap();
    let snapshot = KernelSnapshot::from_context(&decoded).unwrap();
    assert_eq!(
        snapshot.market_states()[0]
            .book
            .as_ref()
            .unwrap()
            .window_min,
        None
    );

    let mut context = with_book();
    context.book_window_min = window(
        vec![level(300_000, 0)],
        vec![level(600_000, 2_000), level(550_000, 4_000)],
    );
    let decoded =
        decode_decision_context_v6(&encode_decision_context_v6(&context).unwrap()).unwrap();
    assert_eq!(decoded, context);
    let snapshot = KernelSnapshot::from_context(&decoded).unwrap();
    let quantity = ContractQuantity::from_hundredths;
    assert_eq!(
        snapshot.market_states()[0]
            .book
            .as_ref()
            .unwrap()
            .window_min,
        Some(BookWindowMin {
            window_ms: 2_000,
            yes_bids: vec![BookLevel {
                price: 0.30,
                quantity: quantity(0),
            }],
            no_bids: vec![
                BookLevel {
                    price: 0.60,
                    quantity: quantity(2_000),
                },
                BookLevel {
                    price: 0.55,
                    quantity: quantity(4_000),
                },
            ],
        })
    );
    // State only, like the book: no part of the fence.
    let mut absent = context.clone();
    absent.book_window_min = None;
    assert_eq!(
        decision_fence_v6_sha256(&absent).unwrap(),
        decision_fence_v6_sha256(&context).unwrap()
    );
}

#[test]
fn window_minimums_are_scoped_level_for_level_and_bounded() {
    let valid = || {
        let mut context = with_book();
        context.book_window_min = window(
            vec![level(300_000, 500)],
            vec![level(600_000, 0), level(550_000, 1)],
        );
        context
    };
    assert_eq!(valid().validate(), Ok(()));
    let mut at_bound = valid();
    at_bound.book_window_min.as_mut().unwrap().window_ms = MAX_BOOK_WINDOW_MS;
    assert_eq!(at_bound.validate(), Ok(()));
    let mutations: [fn(&mut DecisionContextV6); 8] = [
        |context| context.book_window_min.as_mut().unwrap().window_ms = 0,
        |context| context.book_window_min.as_mut().unwrap().window_ms = MAX_BOOK_WINDOW_MS + 1,
        |context| context.book_window_min.as_mut().unwrap().markets.clear(),
        |context| {
            context.book_window_min.as_mut().unwrap().markets[0].market_id = "unrelated".to_owned();
        },
        // Above the level's quantity now.
        |context| {
            context.book_window_min.as_mut().unwrap().markets[0].yes_bids[0] = level(300_000, 501)
        },
        // Another price than the book's level.
        |context| {
            context.book_window_min.as_mut().unwrap().markets[0].no_bids[1] = level(540_000, 1)
        },
        // A level missing.
        |context| {
            context.book_window_min.as_mut().unwrap().markets[0]
                .no_bids
                .pop();
        },
        // Levels for a Market without a book.
        |context| context.owner_state.markets[0].book = None,
    ];
    for mutate in mutations {
        let mut invalid = valid();
        mutate(&mut invalid);
        assert_eq!(invalid.validate(), Err(DecisionV6Error::InvalidContract));
    }
}
