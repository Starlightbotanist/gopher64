use crate::{custom_driver, ui};
use slint::{ComponentHandle, Model};

pub fn init(app: &ui::gui::AppWindow, config: &ui::config::Config) {
    app.set_custom_driver_supported(custom_driver::supports_custom_drivers());
    refresh(app, &config.video.custom_driver, String::new());

    let weak = app.as_weak();
    app.on_custom_driver_selected(move |index| {
        weak.upgrade_in_event_loop(move |handle| {
            handle.set_custom_driver_details(details_for_index(&handle, index).into());
            handle.set_custom_driver_status(
                "Restart Gopher64 to apply the GPU driver change.".into(),
            );
            ui::gui::save_settings(&handle);
        })
        .unwrap();
    });

    let weak = app.as_weak();
    app.on_install_custom_driver_clicked(move || {
        let weak = weak.clone();
        tokio::spawn(async move {
            let Some(uri) = ui::android::select_gpu_driver().await else {
                return;
            };
            let result = ui::android::get_file_from_uri(&uri)
                .ok_or_else(|| "The selected file could not be opened.".to_string())
                .and_then(|file| custom_driver::install(file));

            weak.upgrade_in_event_loop(move |handle| match result {
                Ok(driver_id) => {
                    refresh(
                        &handle,
                        &driver_id,
                        "Driver installed and selected. Restart Gopher64 to use it.".into(),
                    );
                    ui::gui::save_settings(&handle);
                }
                Err(error) => {
                    handle.set_custom_driver_status(format!("Installation failed: {error}").into());
                }
            })
            .unwrap();
        });
    });

    let weak = app.as_weak();
    app.on_remove_custom_driver_clicked(move || {
        weak.upgrade_in_event_loop(move |handle| {
            let index = handle.get_custom_driver_index();
            let Some(driver_id) = handle.get_custom_driver_ids().row_data(index as usize) else {
                return;
            };
            if driver_id.is_empty() {
                return;
            }
            match custom_driver::remove(driver_id.as_str()) {
                Ok(()) => {
                    refresh(
                        &handle,
                        "",
                        "Driver removed. Restart Gopher64 to use the system Vulkan driver.".into(),
                    );
                    ui::gui::save_settings(&handle);
                }
                Err(error) => {
                    handle.set_custom_driver_status(format!("Removal failed: {error}").into());
                }
            }
        })
        .unwrap();
    });
}

fn refresh(app: &ui::gui::AppWindow, selected_id: &str, status: String) {
    let drivers = custom_driver::list_installed();
    let mut names = vec![slint::SharedString::from("System Vulkan driver")];
    let mut ids = vec![slint::SharedString::new()];
    names.extend(
        drivers
            .iter()
            .map(|driver| slint::SharedString::from(driver.metadata.name.as_str())),
    );
    ids.extend(
        drivers
            .iter()
            .map(|driver| slint::SharedString::from(driver.id.as_str())),
    );
    let selected_index = drivers
        .iter()
        .position(|driver| driver.id == selected_id)
        .map_or(0, |index| index + 1) as i32;

    app.set_custom_driver_names(slint::ModelRc::from(std::rc::Rc::new(
        slint::VecModel::from(names),
    )));
    app.set_custom_driver_ids(slint::ModelRc::from(std::rc::Rc::new(
        slint::VecModel::from(ids),
    )));
    app.set_custom_driver_index(selected_index);
    app.set_custom_driver_details(details_for_index(app, selected_index).into());
    app.set_custom_driver_status(status.into());
}

fn details_for_index(app: &ui::gui::AppWindow, index: i32) -> String {
    let Some(driver_id) = app.get_custom_driver_ids().row_data(index as usize) else {
        return String::new();
    };
    if driver_id.is_empty() {
        return "Uses the Vulkan driver supplied by the device manufacturer.".into();
    }
    let Some(driver) = custom_driver::list_installed()
        .into_iter()
        .find(|driver| driver.id == driver_id.as_str())
    else {
        return String::new();
    };

    let mut details = Vec::new();
    if !driver.metadata.author.is_empty() {
        details.push(format!("Author: {}", driver.metadata.author));
    }
    let version = if driver.metadata.driver_version.is_empty() {
        &driver.metadata.package_version
    } else {
        &driver.metadata.driver_version
    };
    if !version.is_empty() {
        details.push(format!("Version: {version}"));
    }
    if !driver.metadata.description.is_empty() {
        details.push(driver.metadata.description);
    }
    details.join("\n")
}
