//! The script allows us to create the trajs for the swe bench submission and the necessary
//! data which we need to showcase the end result for the submission

use colored::Colorize;
use gix::bstr::ByteSlice;
use std::fs;
use std::io::Write;
use std::{collections::HashSet, fs::File, io::BufWriter, path::PathBuf, sync::Arc};

use clap::Parser;
use futures::StreamExt;
use llm_client::{
    broker::LLMBroker,
    clients::types::LLMType,
    provider::{GoogleAIStudioKey, LLMProvider, LLMProviderAPIKeys},
};
use sidecar::{
    agentic::{
        symbol::{identifier::LLMProperties, tool_box::ToolBox},
        tool::{
            broker::{ToolBroker, ToolBrokerConfiguration},
            code_edit::models::broker::CodeEditBroker,
        },
    },
    chunking::{editor_parsing::EditorParsing, languages::TSLanguageParsing},
    inline_completion::symbols_tracker::SymbolTrackerInline,
    mcts::{
        action_node::{SearchTree, SearchTreeMinimal},
        selector::selector::Selector,
    },
};

use tokio_stream::wrappers::ReadDirStream;

#[derive(Parser, Debug)]
#[command(
    author = "skcd",
    version = "1.0",
    about = "SWE-Bench submission creator"
)]
struct CLIArgs {
    swe_bench_logs_path: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = CLIArgs::parse();

    // setup for the search-tree
    let editor_parsing = Arc::new(EditorParsing::default());
    let symbol_broker = Arc::new(SymbolTrackerInline::new(editor_parsing.clone()));
    let llm_broker = Arc::new(LLMBroker::new().await.expect("to initialize properly"));
    let tool_broker = Arc::new(
        ToolBroker::new(
            llm_broker.clone(),
            Arc::new(CodeEditBroker::new()),
            symbol_broker.clone(),
            Arc::new(TSLanguageParsing::init()),
            ToolBrokerConfiguration::new(None, true),
            LLMProperties::new(
                LLMType::GeminiPro,
                LLMProvider::GoogleAIStudio,
                LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new("".to_owned())),
            ),
        )
        .await,
    );

    let tool_box = Arc::new(ToolBox::new(tool_broker, symbol_broker, editor_parsing));

    let selector = Selector::new(
        1.0,    // exploitation_weight
        false,  // use_average_reward
        1.0,    // exploration_weight
        0.8,    // depth_weight
        0.0,    // depth_bonus_factor
        50.0,   // high_value_threshold
        0.0,    // low_value_threshold
        75.0,   // very_high_value_threshold
        50.0,   // high_value_leaf_bonus_constant
        20.0,   // high_value_bad_children_bonus_constant
        5.0,    // high_value_child_penalty_constant
        50.0,   // finished_trajectory_penalty
        50.0,   // expect_correction_bonus
        vec![], // check_for_bad_child_actions
        100.0,  // diversity_weight
        25.0,   // duplicate_child_penalty_constant
        50.0,   // duplicate_action_penalty_constant
    );

    let swe_bench_logs_path = args.swe_bench_logs_path;

    let directories = ReadDirStream::new(
        tokio::fs::read_dir(swe_bench_logs_path)
            .await
            .expect("to work"),
    )
    .collect::<Vec<_>>()
    .await;

    // now that we have all the directories we can recruse through it
    let mut total_resolved_count = 0;
    let mut incorrect_tree_count = 0;
    let mut fucked_counter = 0;
    let rerun_counter = 0;
    let to_run: HashSet<String> = Default::default();

    let mut all_preds_content: Vec<serde_json::Value> = vec![];

    for directory in directories.into_iter() {
        assert!(directory.is_ok());
        let directory_path = directory.expect("is_ok to hold");

        // now read the directories which are part of the path over here
        let directory_path = directory_path.path();
        let instance_name = directory_path
            .file_name()
            .expect("to be present")
            .to_string_lossy()
            .to_string();
        println!("instance_name: {}", &instance_name);
        let instances = ReadDirStream::new(
            tokio::fs::read_dir(directory_path.clone())
                .await
                .expect("to work"),
        )
        .collect::<Vec<_>>()
        .await;

        let mut resolved = false;
        let mut incorrect_tree_instance_ids: HashSet<String> = Default::default();
        let mut passing_instance_ids: HashSet<String> = Default::default();
        // the last entry here has the patch.diff for the submission
        let mut tree_with_reward: Vec<((String, bool, SearchTree, String), f32)> = vec![];

        for run_instance in instances.into_iter() {
            assert!(run_instance.is_ok());
            let run_instance_entry = run_instance.expect("is_ok to work");
            let run_instance_file_path = run_instance_entry.path();
            let run_instance_id = run_instance_file_path
                .file_name()
                .expect("to be present")
                .to_string_lossy()
                .to_string();
            println!("  run_instance: {:?}", run_instance_id);

            // read the mcts tree over here and check its a sane one
            let mini_mcts_tree_data =
                run_instance_file_path.join(format!("mcts-{}.json", run_instance_id));

            let parsed_mcts_tree = serde_json::from_slice::<SearchTreeMinimal>(
                tokio::fs::read(&mini_mcts_tree_data)
                    .await
                    .expect("to work")
                    .as_slice(),
            )
            .expect("to work");

            let search_tree = SearchTree::from_minimal_tree(
                parsed_mcts_tree,
                selector.clone(),
                llm_broker.clone(),
                tool_box.clone(),
                vec![],
            );

            let tree_score = search_tree.calculate_tree_reward();

            let incorrect_tree = search_tree.check_if_tree_has_branching();
            if incorrect_tree {
                println!("{}", format!("incorrect-tree-found").red());
            }

            // inside each run instance we have the mcts tree along with the report.json which we want to load
            // and look at the resolved: true entry or false otherwise
            let report_path = run_instance_file_path.join("report.json");
            let report_json_content = tokio::fs::read(report_path).await.expect("to never fail");
            let serialized_content =
                serde_json::from_slice::<serde_json::Value>(&report_json_content).expect("to work");

            // now this can be flaky but we know how the json looks like so we should
            // be okay
            let resolved_value = serialized_content
                .get(&instance_name)
                .expect("to be present")
                .get("resolved")
                .expect("to always work");
            let resolved_value = resolved_value.as_bool().expect("to work");
            if resolved_value {
                // how many instances resolved
                passing_instance_ids.insert(run_instance_id.to_owned());
                resolved = true;
            }

            if resolved_value && incorrect_tree {
                incorrect_tree_instance_ids.insert(run_instance_id.to_owned());
                incorrect_tree_count = incorrect_tree_count + 1;
            }

            let patch_content = tokio::fs::read(run_instance_file_path.join("patch.diff"))
                .await
                .expect("to always work")
                .to_str_lossy()
                .to_string();

            tree_with_reward.push((
                (run_instance_id, resolved_value, search_tree, patch_content),
                tree_score,
            ));
        }

        incorrect_tree_instance_ids
            .into_iter()
            .for_each(|instance_id| {
                passing_instance_ids.remove(&instance_id);
            });

        if passing_instance_ids.is_empty() && resolved {
            fucked_counter = fucked_counter + 1;
            println!("{}", format!("we_have_a_problem").red());
        }

        let tree_with_reward_cloned = tree_with_reward.to_vec();
        tree_with_reward.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        // Copying over the files to the right directory over here
        let files_required = vec![
            "eval.sh",
            "patch.diff",
            "report.json",
            "run_instance.log",
            "test_output.txt",
        ];

        // now we also want to create the trajs file
        // for the trajs we will do the following:
        // - cap the maximum results we will show in the instance_id to 5
        // - for the traj itself we also want to show the mcts tree at the very top
        // followed by all the steps the agent took and verbosly
        // - as a separator for the trajs we will put =================================== START OF TRAJECTORY ===================================
        // and then =================================== END OF TRAJECTORY ===================================

        let mut traj_content = vec![];

        if resolved {
            // pick the first instance here after sorting
            let tree_instance = tree_with_reward.get(0).expect("to exist");
            let instance_run_id = &tree_instance.0 .0;
            let patch = &tree_instance.0 .3;
            all_preds_content.push(serde_json::json!({
                "instance_id": instance_name.to_owned(),
                "model_patch": patch.to_owned(),
                "model_name_or_path": "codestory-midwit".to_owned(),
            }));

            // pick the instance which is passing and then deep copy the files in that folder to our destination folder
            let directory_to_deep_copy = directory_path.clone().join(instance_run_id.to_owned());
            for file_required in files_required.to_vec().into_iter() {
                let file_path = directory_to_deep_copy.clone().join(file_required);
                let destination = format!("/Users/skcd/test_repo/experiments/evaluation/verified/20241213_codestory_midwit_claude-3-5-sonnet/logs/{}", instance_name);

                fs::create_dir_all(&destination).expect("to work");
                // now copy the content over
                let destination = format!("/Users/skcd/test_repo/experiments/evaluation/verified/20241213_codestory_midwit_claude-3-5-sonnet/logs/{}/{}", instance_name, file_required);
                fs::copy(&file_path, &destination).expect("to work");
            }

            let mut trajectory_maximum_counter = 0;
            tree_with_reward.iter().for_each(
                |((_instance_id, _resolved, search_tree, _patch_diff), _score)| {

                    // otherwise we can keep going
                    traj_content.push(format!("=================================== START OF TRAJECTORY ==================================="));
                    search_tree.print_tree(&mut traj_content);
                    search_tree.print_midwit_tree(0, &mut traj_content);
                    traj_content.push("=================================== END OF TRAJECTORY ===================================".to_owned());
                    trajectory_maximum_counter = trajectory_maximum_counter + 1;
                },
            );

            // get the index from the original order
            // this is important to highlight the variance in the agent and also
            // to show that the first agent is not the correct one at times
            let selected_entry = tree_with_reward_cloned
                .into_iter()
                .enumerate()
                .find(|(_idx, current_tree_instance)| {
                    current_tree_instance.0 .0 == tree_instance.0 .0
                })
                .expect("to work");

            traj_content.push("=================================== PICKING BEST TRAJECTORY ===================================".to_owned());
            traj_content.push(format!("<traj>{}</traj>", selected_entry.0));
            traj_content.push("=================================== END ITERATION ===================================".to_owned());

            // now write the content of the traj_content to the file
            let file_path = format!("/Users/skcd/test_repo/experiments/evaluation/verified/20241213_codestory_midwit_claude-3-5-sonnet/trajs/{}.log", instance_name.to_owned());
            fs::write(file_path, traj_content.join("\n")).expect("to always work");

            // we already know the files we want to copy so we can just go through the list and copy it to the right location
        } else {
            // pick the first one over here
            let _tree_with_reward_len = tree_with_reward.len();
            let tree_instance = tree_with_reward.get(0).expect("to exist");
            let patch = &tree_instance.0 .3;
            let instance_run_id = &tree_instance.0 .0;
            all_preds_content.push(serde_json::json!({
                "instance_id": instance_name.to_owned(),
                "model_patch": patch.to_owned(),
                "model_name_or_patch": "codestory-midwit".to_owned(),
            }));

            // pick the instance which is passing and then deep copy the files in that folder to our destination folder
            let directory_to_deep_copy = directory_path.clone().join(instance_run_id.to_owned());
            for file_required in files_required.to_vec().into_iter() {
                let file_path = directory_to_deep_copy.clone().join(file_required);
                let destination = format!("/Users/skcd/test_repo/experiments/evaluation/verified/20241213_codestory_midwit_claude-3-5-sonnet/logs/{}", instance_name);

                fs::create_dir_all(&destination).expect("to work");
                // now copy the content over
                let destination = format!("/Users/skcd/test_repo/experiments/evaluation/verified/20241213_codestory_midwit_claude-3-5-sonnet/logs/{}/{}", instance_name, file_required);
                fs::copy(&file_path, &destination).expect("to work");
            }

            let mut trajectory_maximum_counter = 0;
            tree_with_reward.iter().for_each(
                |((_instance_id, _resolved, search_tree, _patch_diff), _score)| {
                    if trajectory_maximum_counter >= 5 {
                        return;
                    }

                    // otherwise we can keep going
                    traj_content.push(format!("=================================== START OF TRAJECTORY ==================================="));
                    search_tree.print_tree(&mut traj_content);
                    search_tree.print_midwit_tree(0, &mut traj_content);
                    traj_content.push("=================================== END OF TRAJECTORY ===================================".to_owned());
                    trajectory_maximum_counter = trajectory_maximum_counter + 1;
                },
            );

            traj_content.push("=================================== PICKING BEST TRAJECTORY ===================================".to_owned());
            traj_content.push(format!("<traj>0</traj>"));
            traj_content.push("=================================== END ITERATION ===================================".to_owned());

            // now write the content of the traj_content to the file
            let file_path = format!("/Users/skcd/test_repo/experiments/evaluation/verified/20241213_codestory_midwit_claude-3-5-sonnet/trajs/{}.log", instance_name.to_owned());
            fs::write(file_path, traj_content.join("\n")).expect("to always work");
        }
        // assert!(tree_with_reward.get(0).expect("to work").0 .1);

        if resolved {
            total_resolved_count = total_resolved_count + 1;
        }
        // doing this for one instance
        // break;
    }

    println!(
        "total_resolved:{}\nincorrect_tree:{}\nfucked_counter:{}\n:rerun_counter:{}",
        total_resolved_count, incorrect_tree_count, fucked_counter, rerun_counter
    );

    println!(
        "run_ids:\n{}",
        to_run.into_iter().collect::<Vec<_>>().join("\n")
    );

    let file = File::create("/Users/skcd/test_repo/experiments/evaluation/verified/20241213_codestory_midwit_claude-3-5-sonnet/all_preds.jsonl")?;
    let mut writer = BufWriter::new(file);

    // Write each value on its own line
    for value in all_preds_content {
        serde_json::to_writer(&mut writer, &value)?;
        writeln!(writer)?; // Add newline after each JSON object
    }

    // let content = tokio::fs::read(args.mcts_tree_path)
    //     .await
    //     .expect("reading file should work with correct args");

    // let editor_parsing = Arc::new(EditorParsing::default());
    // let symbol_broker = Arc::new(SymbolTrackerInline::new(editor_parsing.clone()));
    // let llm_broker = Arc::new(
    //     LLMBroker::new(LLMBrokerConfiguration::new(default_index_dir()))
    //         .await
    //         .expect("to initialize properly"),
    // );
    // let tool_broker = Arc::new(ToolBroker::new(
    //     llm_broker.clone(),
    //     Arc::new(CodeEditBroker::new()),
    //     symbol_broker.clone(),
    //     Arc::new(TSLanguageParsing::init()),
    //     ToolBrokerConfiguration::new(None, true),
    //     LLMProperties::new(
    //         LLMType::GeminiPro,
    //         LLMProvider::GoogleAIStudio,
    //         LLMProviderAPIKeys::GoogleAIStudio(GoogleAIStudioKey::new("".to_owned())),
    //     ),
    // ));

    // let tool_box = Arc::new(ToolBox::new(tool_broker, symbol_broker, editor_parsing));

    // let selector = Selector::new(
    //     1.0,    // exploitation_weight
    //     false,  // use_average_reward
    //     1.0,    // exploration_weight
    //     0.8,    // depth_weight
    //     0.0,    // depth_bonus_factor
    //     50.0,   // high_value_threshold
    //     0.0,    // low_value_threshold
    //     75.0,   // very_high_value_threshold
    //     50.0,   // high_value_leaf_bonus_constant
    //     20.0,   // high_value_bad_children_bonus_constant
    //     5.0,    // high_value_child_penalty_constant
    //     50.0,   // finished_trajectory_penalty
    //     50.0,   // expect_correction_bonus
    //     vec![], // check_for_bad_child_actions
    //     100.0,  // diversity_weight
    //     25.0,   // duplicate_child_penalty_constant
    //     50.0,   // duplicate_action_penalty_constant
    // );

    // let search_tree_minimal = serde_json::from_slice::<SeachTreeMinimal>(content.as_slice())
    //     .expect("search_tree_minimal_to_not_fail");

    // let search_tree =
    //     SearchTree::from_minimal_tree(search_tree_minimal, selector, llm_broker, tool_box);

    // search_tree.print_tree();

    Ok(())
}
