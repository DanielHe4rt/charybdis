pub mod data;
mod runner;

use crate::migration_unit::data::MigrationUnitData;
use crate::migration_unit::runner::MigrationUnitRunner;
use colored::Colorize;
use scylla::Session;
use std::fmt::Display;

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub(crate) enum MigrationObjectType {
    Udt,
    Table,
    MaterializedView,
}

impl Display for MigrationObjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationObjectType::Udt => write!(f, "UDT"),
            MigrationObjectType::Table => write!(f, "Table"),
            MigrationObjectType::MaterializedView => write!(f, "Materialized View"),
        }
    }
}

pub(crate) struct MigrationUnit<'a> {
    data: &'a MigrationUnitData<'a>,
    runner: MigrationUnitRunner<'a>,
    drop_and_replace: bool,
}

impl<'a> MigrationUnit<'a> {
    pub(crate) fn new(data: &'a MigrationUnitData, session: &'a Session, drop_and_replace: bool) -> Self {
        let runner = MigrationUnitRunner::new(&session, &data);

        Self {
            data,
            runner,
            drop_and_replace,
        }
    }

    pub(crate) async fn run(&self) {
        if self.data.is_first_migration() {
            self.handle_first_migration().await;
            return;
        }

        self.runner.run_table_options_change_migration().await;

        self.panic_on_partition_key_change();
        self.panic_on_clustering_key_change();

        let mut is_any_field_changed = false;

        if self.data.has_changed_type_fields() {
            is_any_field_changed = true;
            self.handle_fields_type_change().await;
        }

        if self.data.has_new_fields() {
            is_any_field_changed = true;
            self.handle_new_fields().await;
        }

        if self.data.has_removed_fields() {
            is_any_field_changed = true;
            self.handle_removed_fields().await;
        }

        if self.data.has_new_global_secondary_indexes() {
            is_any_field_changed = true;
            self.runner.run_global_index_added_migration().await;
        }

        if self.data.has_new_local_secondary_indexes() {
            is_any_field_changed = true;
            self.runner.run_local_index_added_migration().await;
        }

        if self.data.has_removed_global_secondary_indexes() {
            is_any_field_changed = true;
            self.runner.run_global_index_removed_migration().await;
        }

        if self.data.has_removed_local_secondary_indexes() {
            is_any_field_changed = true;
            self.runner.run_local_index_removed_migration().await;
        }

        if !is_any_field_changed {
            println!(
                "{} {} {}",
                "No changes detected in".green(),
                self.data.migration_object_name.bright_yellow(),
                self.data.migration_object_type.to_string().bright_magenta()
            );
        }
    }

    async fn handle_first_migration(&self) {
        self.runner.run_first_migration().await;

        if self.data.has_new_global_secondary_indexes() {
            self.runner.run_global_index_added_migration().await;
        }

        if self.data.has_new_local_secondary_indexes() {
            self.runner.run_local_index_added_migration().await;
        }
    }

    async fn handle_fields_type_change(&self) {
        if self.drop_and_replace {
            self.panic_on_mv_fields_change();
            self.panic_on_udt_fields_removal();

            self.runner.run_field_type_changed_migration().await;
        } else {
            self.panic_on_field_type_change();
        }
    }

    async fn handle_new_fields(&self) {
        self.panic_on_mv_fields_change();

        self.runner.run_field_added_migration().await;
    }

    async fn handle_removed_fields(&self) {
        self.panic_on_mv_fields_change();
        self.panic_on_udt_fields_removal();

        self.runner.run_field_removed_migration().await;
    }

    fn panic_on_field_type_change(&self) {
        panic!(
            "\n\n{} {} {}\n{}\n\n",
            "Illegal change in".bright_red(),
            self.data.migration_object_name.bright_yellow(),
            self.data.migration_object_type.to_string().bright_magenta(),
            "Field type change is not allowed. Use `-d` flag to drop and recreate field with new type!".bright_red(),
        );
    }

    fn panic_on_partition_key_change(&self) {
        if self.data.migration_object_type != MigrationObjectType::Udt {
            if self.data.partition_key_changed() {
                panic!(
                    "\n\n{} {} {}\n{}\n\n",
                    "Illegal change in".bright_red(),
                    self.data.migration_object_name.bright_yellow(),
                    self.data.migration_object_type.to_string().bright_magenta(),
                    "Partition key change is not allowed!".bright_red(),
                );
            }
        }
    }

    fn panic_on_clustering_key_change(&self) {
        if self.data.migration_object_type != MigrationObjectType::Udt {
            if self.data.clustering_key_changed() {
                panic!(
                    "\n\n{} {} {}\n{}\n\n",
                    "Illegal change in".bright_red(),
                    self.data.migration_object_name.bright_yellow(),
                    self.data.migration_object_type.to_string().bright_magenta(),
                    "Clustering key change is not allowed!".bright_red(),
                );
            }
        }
    }

    fn panic_on_udt_fields_removal(&self) {
        if self.data.migration_object_type == MigrationObjectType::Udt
            && (self.data.has_removed_fields() || self.data.has_changed_type_fields())
        {
            panic!("\n{}\n", "UDT fields removal is not allowed!".bold().bright_red());
        }
    }

    fn panic_on_mv_fields_change(&self) {
        if self.data.migration_object_type == MigrationObjectType::MaterializedView {
            panic!(
                "\n{}\n",
                "Materialized view fields change is not allowed!".bold().bright_red()
            );
        }
    }
}
