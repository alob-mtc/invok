use db_entities::prelude::Function;
use db_entities::{
    auth::Column as AuthColumn,
    function::{ActiveModel as FunctionModel, Column, Model},
    prelude::Auth as AuthEntity,
};
use db_migrations::Condition;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, DbConn, EntityTrait, QueryFilter};
use uuid::Uuid;

pub struct FunctionDBRepo;

impl FunctionDBRepo {
    /// Finds a function by its name in the database.
    ///
    /// # Arguments
    ///
    /// * `conn` - A reference to the database connection.
    /// * `name` - The name of the function to find.
    ///
    /// # Returns
    ///
    /// * `Some(Model)` if the function exists; otherwise, `None`.
    pub async fn find_function_by_name(
        conn: &DbConn,
        name: &str,
        user_uuid: Uuid,
    ) -> Option<Model> {
        Function::find()
            .filter(
                Condition::all()
                    .add(Column::Name.eq(name))
                    .add(Column::Uuid.eq(user_uuid)),
            )
            .one(conn)
            .await
            .ok()?
    }

    /// Finds functions by user's UUID in the database.
    ///
    /// # Arguments
    ///
    /// * `conn` - A reference to the database connection.
    /// * `user_uuid` - The UUID of the user.
    ///
    /// # Returns
    ///
    /// * Vector of functions belonging to the user
    pub async fn find_functions_by_user_uuid(
        conn: &DbConn,
        user_uuid: Uuid,
    ) -> Result<Vec<Model>, sea_orm::DbErr> {
        // First find the user by UUID
        let user = AuthEntity::find()
            .filter(AuthColumn::Uuid.eq(user_uuid))
            .one(conn)
            .await?;

        // If no user found, return empty list
        let user = match user {
            Some(user) => user,
            None => return Ok(vec![]),
        };

        // Find all functions for this user
        Function::find()
            .filter(Column::AuthId.eq(user.id))
            .all(conn)
            .await
    }

    /// Creates a new function in the database for a specific user.
    ///
    /// # Arguments
    ///
    /// * `conn` - A reference to the database connection.
    /// * `function` - The function model to insert.
    /// * `user_uuid` - The UUID of the user who owns this function.
    ///
    /// # Returns
    ///
    /// * `Ok(())` on success, or an error of type `sea_orm::DbErr` if insertion fails.
    pub async fn create_function_for_user(
        conn: &DbConn,
        function: Model,
        user_uuid: Uuid,
    ) -> Result<Model, sea_orm::DbErr> {
        // Find the user by UUID
        let user = AuthEntity::find()
            .filter(AuthColumn::Uuid.eq(user_uuid))
            .one(conn)
            .await?
            .ok_or_else(|| sea_orm::DbErr::Custom("User not found".to_string()))?;

        // Create the function model record with the user's ID
        let function_model = FunctionModel {
            auth_id: Set(user.id),
            name: Set(function.name),
            runtime: Set(function.runtime),
            uuid: Set(user_uuid),
            ..Default::default()
        };

        // Insert and return the created function
        function_model.insert(conn).await
    }
}
