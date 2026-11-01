use super::*;
use crate::settings::DestinationSettings;

#[derive(Template)]
#[template(path = "settings.html")]
struct Settings {
    csrf: String,
    settings: DestinationSettings,
    saved: bool,
}

#[derive(Default, Deserialize)]
pub(super) struct SettingsQuery {
    #[serde(default)]
    saved: bool,
}

pub(super) async fn page(
    State(state): State<AppState>,
    Query(query): Query<SettingsQuery>,
) -> Result<Html<String>> {
    render(Settings {
        csrf: state.0.csrf.clone(),
        settings: state.work(|s| s.destination_settings()).await?,
        saved: query.saved,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SettingsForm {
    csrf: String,
    version: i64,
    personal_account: String,
    personal_pot: String,
    joint_account: String,
    joint_pot: String,
}

pub(super) async fn save(
    State(state): State<AppState>,
    Form(form): Form<SettingsForm>,
) -> Result<Redirect> {
    state.check_csrf(&form.csrf)?;
    state
        .work(move |s| {
            s.save_destination_settings(DestinationSettings {
                version: form.version,
                personal_account: form.personal_account,
                personal_pot: form.personal_pot,
                joint_account: form.joint_account,
                joint_pot: form.joint_pot,
            })
        })
        .await?;
    Ok(Redirect::to("/settings?saved=true"))
}
